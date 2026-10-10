//! 数式モードの評価
//!
//! インライン数式と数式環境のセルを [`HirMath`] 列に変換する。
//!
//! コマンドの数式引数（[`math_arg_to_node`]）のように単一ノードへ畳まれて、先に確保したグループ用の ID が使われない場合は
//! `NodeId` に穴が空くが、同じ入力なら常に同じ穴になるので決定性は保たれる。

use crate::{
  document::{HirMath, HirMathKind, MathVariant, NodeId},
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity,
      command::symbol::{self, MathSymbol},
      opt_args,
    },
    syntax::{
      CstElement, CstNode, SyntaxKind,
      token::TokenKind,
      view::{CommandView, EnvironmentView},
    },
  },
};

/// 数式モードで構造化された CST ノードの子要素を [`HirMath`] 列に変換する
///
/// # Errors
///
/// 数式内のコマンドが不正な引数数を持つ場合などにエラーを返します。
pub(super) fn evaluate_math_children(
  source: &str,
  ctx: &EvalContext<'_>,
  node: &CstNode<'_>,
) -> Result<Vec<HirMath>, EvalError> {
  return evaluate_math_elements(source, ctx, node.children);
}

/// 数式モードで構造化された要素列を [`HirMath`] 列に変換する共通ヘルパ
pub(crate) fn evaluate_math_elements(
  source: &str,
  ctx: &EvalContext<'_>,
  elements: &[CstElement<'_>],
) -> Result<Vec<HirMath>, EvalError> {
  let mut nodes = Vec::new();
  for child in elements {
    match child {
      CstElement::Token(token) => match token.kind {
        // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする。
        TokenKind::Text
        | TokenKind::VerbatimText
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::Whitespace
        | TokenKind::Newline => {
          nodes.push(ctx.leaf_math(token.span, HirMathKind::Text(token.text(source).to_string())));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(ctx.leaf_math(token.span, HirMathKind::Text(text.to_string())));
        },
        TokenKind::Ampersand => {
          return Err(EvalError::UnsupportedInMath {
            what: "&（列区切り）".to_string(),
            span: token.span.into(),
          });
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（行区切り）".to_string(),
            span: token.span.into(),
          });
        },
        // 構造トークン（コマンド・括弧類・`$`・上下付きマーカー）と段落区切り・コメント・
        // 不正トークンは数式に残さない。意味を持つ実体は parser がノードへ畳んだ側にある。
        TokenKind::Command
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::Dollar
        | TokenKind::Underscore
        | TokenKind::Caret
        | TokenKind::ParagraphBreak
        | TokenKind::Comment
        | TokenKind::Unknown => {},
      },
      CstElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, ctx, child_node)?;
          nodes.push(math_node);
        },
        SyntaxKind::MathGroup => {
          let id = ctx.alloc(child_node.span);
          let inner = evaluate_math_children(source, ctx, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Group(inner)));
        },
        SyntaxKind::MathSubscript => {
          let id = ctx.alloc(child_node.span);
          let content = evaluate_math_script_content(source, ctx, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Subscript(Box::new(content))));
        },
        SyntaxKind::MathSuperscript => {
          let id = ctx.alloc(child_node.span);
          let content = evaluate_math_script_content(source, ctx, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Superscript(Box::new(content))));
        },
        SyntaxKind::Environment => {
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::UnsupportedInMath {
            what: format!("環境 {}", view.name()),
            span: child_node.span.into(),
          });
        },
        // 引数・環境タグはそれぞれの評価経路が中身を取り出す。`InlineMath` は数式の入れ子で、
        // 数式本体の直下で出会っても数式ノードとしては扱わない。
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::InlineMath => {},
      },
    }
  }
  return Ok(nodes);
}

/// 上付き・下付きスクリプトノードの中身を単一の [`HirMath`] に変換する
///
/// `parse_math_script`（`syntax::parser`）が内容を `{...}` グループ 1 個に限定しているので、
/// 子は `^` / `_` 自身・先行トリビアのトークンと、内容の `MathGroup` ノード 1 個だけになる。
fn evaluate_math_script_content(
  source: &str,
  ctx: &EvalContext<'_>,
  script_node: &CstNode<'_>,
) -> Result<HirMath, EvalError> {
  let group_node = script_node.children.iter().find_map(|child| {
    return match child {
      CstElement::Node(node) if node.kind == SyntaxKind::MathGroup => Some(node),
      CstElement::Node(_) | CstElement::Token(_) => None,
    };
  });
  let Some(group_node) = group_node else {
    unreachable!(
      "`parse_math_script` は内容が `{{...}}` グループでなければ構文エラーにするので、スクリプトノードには必ず MathGroup の子がある"
    )
  };

  let id = ctx.alloc(group_node.span);
  let inner = evaluate_math_children(source, ctx, group_node)?;
  return Ok(HirMath::new(id, HirMathKind::Group(inner)));
}

/// ノード列を単一ノードへ畳む（1 個ならそのまま、それ以外は予約済み ID のグループにする）
fn collapse_single(group_id: NodeId, nodes: Vec<HirMath>) -> HirMath {
  if nodes.len() == 1 {
    let Some(single) = nodes.into_iter().next() else {
      unreachable!("長さ 1 を確認した直後なので必ず要素がある")
    };
    return single;
  }
  return HirMath::new(group_id, HirMathKind::Group(nodes));
}

/// 数式内コマンドの種類
///
/// 数式の語彙（字形コマンド・`\frac`・`\sqrt`・伸縮括弧・アクセント・上下線・記号表）を名前から 1 回だけ引いた結果。必須引数の個数は
/// [`Self::arg_count`] の網羅 match 1 箇所で宣言し、パーサーが数式内で引数を読む上限（[`lookup_math_arg_count`]
/// 経由）と評価の個数検査（[`evaluate_math_command`] の `arity` 呼び出し）の両方がこの個数に従う。
/// variant を足すと `arg_count` がコンパイルエラーで個数の宣言を求める。
#[derive(Debug, Clone, Copy)]
enum MathCommandKind {
  /// 字形コマンド（`\mathbold` 等）— 必須引数 1 個（数式本体）
  Styled(MathVariant),
  /// `\frac` — 必須引数 2 個（分子と分母）
  Frac,
  /// `\sqrt` — 必須引数 1 個（被開平数）。根指数は任意引数なので数えない
  Sqrt,
  /// 伸縮括弧（`\paren` 等）— 必須引数 1 個（括弧で包む数式）
  Fenced {
    /// 左の区切り括弧
    open: char,
    /// 右の区切り括弧
    close: char,
  },
  /// アクセント（`\hat` 等・`\widehat` 等）— 必須引数 1 個（基底）
  Accent {
    /// 基底の上に置く結合用ダイアクリティカルマーク
    mark: char,
    /// 記号を基底の送り幅へ横に伸ばすか（広幅アクセント）
    wide: bool,
  },
  /// 上線・下線（`\overline` / `\underline`）— 必須引数 1 個（線を引く数式）
  Bar {
    /// 線を基底の上に引くか（上線は `true`、下線は `false`）
    over: bool,
  },
  /// 記号コマンド（`\alpha` 等）— 必須引数なし
  Symbol(MathSymbol),
}

impl MathCommandKind {
  /// コマンド名から数式の語彙を引く。数式の語彙に無ければ `None`
  fn from_name(name: &str) -> Option<Self> {
    if let Some(variant) = MathVariant::from_command_name(name) {
      return Some(Self::Styled(variant));
    }
    return match name {
      "frac" => Some(Self::Frac),
      "sqrt" => Some(Self::Sqrt),
      _ => fence_delimiters(name)
        .map(|(open, close)| return Self::Fenced { open, close })
        .or_else(|| return accent_mark(name).map(|mark| return Self::Accent { mark, wide: false }))
        .or_else(|| return wide_accent_mark(name).map(|mark| return Self::Accent { mark, wide: true }))
        .or_else(|| return bar_side(name).map(|over| return Self::Bar { over }))
        .or_else(|| return symbol::lookup(name).map(Self::Symbol)),
    };
  }

  /// 必須引数の個数
  fn arg_count(self) -> usize {
    return match self {
      Self::Styled(_) | Self::Sqrt | Self::Fenced { .. } | Self::Accent { .. } | Self::Bar { .. } => 1,
      Self::Frac => 2,
      Self::Symbol(_) => 0,
    };
  }
}

/// 伸縮括弧のコマンド名から、左右の区切り括弧を引く（伸縮括弧でなければ `None`）
///
/// 字は cases / matrix の `delimiter` の括弧と同じ（`\abs` は U+007C、`\norm` は U+2016）。
fn fence_delimiters(name: &str) -> Option<(char, char)> {
  return match name {
    "paren" => Some(('(', ')')),
    "bracket" => Some(('[', ']')),
    "brace" => Some(('{', '}')),
    "abs" => Some(('|', '|')),
    "norm" => Some(('\u{2016}', '\u{2016}')),
    _ => None,
  };
}

/// アクセントコマンド名から、基底の上に置く結合用ダイアクリティカルマークを引く（アクセントでなければ `None`）
///
/// 対応は unicode-math の `\mathaccent`（`unicode-math-table.tex`）と同じ。数式フォントの OpenType MATH は結合記号の側に
/// `MathTopAccentAttachment` と `flac` の字形を持つ（スペーシング形の U+02C6 等は持たない）。
fn accent_mark(name: &str) -> Option<char> {
  return match name {
    "hat" => Some('\u{0302}'),
    "bar" => Some('\u{0304}'),
    "vec" => Some('\u{20D7}'),
    "dot" => Some('\u{0307}'),
    "ddot" => Some('\u{0308}'),
    "tilde" => Some('\u{0303}'),
    "check" => Some('\u{030C}'),
    "acute" => Some('\u{0301}'),
    "grave" => Some('\u{0300}'),
    "breve" => Some('\u{0306}'),
    _ => None,
  };
}

/// 広幅アクセントのコマンド名から、基底の上に置いて基底の送り幅へ横に伸ばす結合用ダイアクリティカルマークを引く
/// （広幅アクセントでなければ `None`）
///
/// `\widehat` / `\widetilde` / `\widecheck` は unicode-math の `\mathaccentwide` と同じ字、矢印は結合用の矢印（上）。
/// 横に伸ばす字形は数式フォントの OpenType MATH が結合記号の側に持つ横方向の size variant と glyph assembly で、持たない
/// フォントでは元の字形のまま組む。
fn wide_accent_mark(name: &str) -> Option<char> {
  return match name {
    "widehat" => Some('\u{0302}'),
    "widetilde" => Some('\u{0303}'),
    "widecheck" => Some('\u{030C}'),
    "overrightarrow" => Some('\u{20D7}'),
    "overleftarrow" => Some('\u{20D6}'),
    "overleftrightarrow" => Some('\u{20E1}'),
    _ => None,
  };
}

/// 上線・下線のコマンド名から、線を基底の上に引くか（`\overline` は `true`、`\underline` は `false`）を引く（上線・下線で
/// なければ `None`）
fn bar_side(name: &str) -> Option<bool> {
  return match name {
    "overline" => Some(true),
    "underline" => Some(false),
    _ => None,
  };
}

/// 数式内のコマンド名から必須引数の個数を引く
///
/// 数式の語彙に無いコマンドは `None` — 評価器が未知のコマンドとして拒否するだけなので個数が定まらず、パーサーは個数で
/// 打ち切らない（レジストリの verbatim 宣言どおりに引数を読み、`$\code{a // b}$` を「未知のコマンド」で
/// 診断できるようにするため）。
pub(super) fn lookup_math_arg_count(name: &str) -> Option<usize> {
  return MathCommandKind::from_name(name).map(MathCommandKind::arg_count);
}

/// 数式内コマンドを [`HirMath`] に変換する
///
/// 数式内ではパーサーが [`MathCommandKind::arg_count`] 個で引数の読みを打ち切るので、
/// 各 arm の `arity` 検査で実際に起きうるのは不足だけになる。
fn evaluate_math_command(source: &str, ctx: &EvalContext<'_>, cmd_node: &CstNode<'_>) -> Result<HirMath, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let Some(kind) = MathCommandKind::from_name(view.name()) else {
    return Err(EvalError::UnknownCommand {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  };

  match kind {
    MathCommandKind::Styled(variant) => {
      opt_args::no_command_opt_args(&view)?;
      let first_arg = arity::exactly_one_arg(&view, "1 個（数式本体）")?;
      let id = ctx.alloc(view.span());
      let children = evaluate_math_children(source, ctx, first_arg)?;
      return Ok(HirMath::new(id, HirMathKind::Styled { variant, children }));
    },
    MathCommandKind::Frac => {
      opt_args::no_command_opt_args(&view)?;
      let (numer_arg, denom_arg) = arity::exactly_two_args(&view, "2 個（分子と分母）")?;
      let id = ctx.alloc(view.span());
      let numer = Box::new(math_arg_to_node(source, ctx, numer_arg)?);
      let denom = Box::new(math_arg_to_node(source, ctx, denom_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Frac { numer, denom }));
    },
    MathCommandKind::Sqrt => {
      // 根指数 `[n]` は任意引数を数式として読む（`no_command_opt_args` は呼ばない）。
      // 個数検査は根指数の評価より前に置く — `math_arg_to_node` は `ctx.alloc` で NodeId を
      // 消費するので、引数の個数が誤っていて後段で reject するだけの入力に対して、その割り当てを
      // 発生させないため。
      let radicand_arg = arity::exactly_one_arg(&view, "1 個（被開平数）")?;
      let id = ctx.alloc(view.span());
      let index = match view.opt_arg() {
        Some(opt) => Some(Box::new(math_arg_to_node(source, ctx, opt)?)),
        None => None,
      };
      let radicand = Box::new(math_arg_to_node(source, ctx, radicand_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Sqrt { index, radicand }));
    },
    MathCommandKind::Fenced { open, close } => {
      opt_args::no_command_opt_args(&view)?;
      let body_arg = arity::exactly_one_arg(&view, "1 個（括弧で包む数式）")?;
      let id = ctx.alloc(view.span());
      let body = Box::new(math_arg_to_node(source, ctx, body_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Fenced { open, close, body }));
    },
    MathCommandKind::Accent { mark, wide } => {
      opt_args::no_command_opt_args(&view)?;
      let base_arg = arity::exactly_one_arg(&view, "1 個（アクセントを付ける数式）")?;
      let id = ctx.alloc(view.span());
      let base = Box::new(math_arg_to_node(source, ctx, base_arg)?);
      return Ok(HirMath::new(
        id,
        HirMathKind::Accent {
          accent: mark,
          wide,
          base,
        },
      ));
    },
    MathCommandKind::Bar { over } => {
      opt_args::no_command_opt_args(&view)?;
      let body_arg = arity::exactly_one_arg(&view, "1 個（線を引く数式）")?;
      let id = ctx.alloc(view.span());
      let body = Box::new(math_arg_to_node(source, ctx, body_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Bar { over, body }));
    },
    MathCommandKind::Symbol(symbol) => {
      opt_args::no_command_opt_args(&view)?;
      arity::no_command_args(&view)?;
      return Ok(ctx.leaf_math(
        view.span(),
        HirMathKind::Symbol {
          ch: symbol.ch,
          class: symbol.class,
        },
      ));
    },
  }
}

/// 数式引数ノードを単一の [`HirMath`] に変換するヘルパー
fn math_arg_to_node(source: &str, ctx: &EvalContext<'_>, arg_node: &CstNode<'_>) -> Result<HirMath, EvalError> {
  let group_id = ctx.alloc(arg_node.span);
  let nodes = evaluate_math_children(source, ctx, arg_node)?;
  return Ok(collapse_single(group_id, nodes));
}
