//! 数式モードの評価
//!
//! インライン数式と数式環境のセルを [`HirMath`] 列に変換する。
//!
//! コマンドの数式引数（[`math_arg_to_node`]）のように単一ノードへ畳まれて、先に確保したグループ用の ID が使われない場合は
//! `NodeId` に穴が空くが、同じ入力なら常に同じ穴になるので決定性は保たれる。

use std::iter;

use icu::normalizer::ComposingNormalizerBorrowed;
use itertools::Itertools;

use crate::{
  document::{HirInlineKind, HirMath, HirMathKind, MathClass, MathSpace, MathVariant, NodeId},
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity, command,
      command::symbol::{self, MathSymbol},
      inline::{self, TokenInline},
      opt_args::{self, OptKey},
    },
    syntax::{
      self, ArgMode, CstElement, CstNode, SyntaxKind,
      token::{Token, TokenKind},
      view::{CommandView, EnvironmentView},
    },
  },
  source::Span,
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
        TokenKind::Text => {
          if let Some(span) = leading_combining_span(source, token) {
            return Err(EvalError::CombiningMarkWithoutBase { span: span.into() });
          }
          push_math_text(ctx, token.text(source), token.span.start, &mut nodes);
        },
        // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする。
        TokenKind::VerbatimText | TokenKind::Comma | TokenKind::Equals | TokenKind::Whitespace | TokenKind::Newline => {
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
        | TokenKind::CombiningMarkOnSyntaxChar
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

/// テキストトークンの先頭に続く結合文字の範囲（先頭が結合文字でなければ `None`）
///
/// 結合文字は直前の字と同じトークンに入る（lexer）ので、テキストトークンが結合文字で始まるのは空白・改行の
/// 直後だけである。数式はそれらを組版に出さないので、先頭の結合文字には付く字が無い。
fn leading_combining_span(source: &str, token: &Token) -> Option<Span> {
  let len: usize = token
    .text(source)
    .chars()
    .take_while(|&ch| return syntax::is_combining(ch))
    .map(char::len_utf8)
    .sum();
  if len == 0 {
    return None;
  }
  let len = u32::try_from(len).expect("トークンはソースの部分文字列で、ソースは u32::MAX バイトを超えない前提");
  return Some(Span::new(token.span.start, token.span.start + len));
}

/// 数式のテキストトークンの中身 `text`（ソース上の開始位置 `start`）を HIR へ積む
///
/// 記号表の字の結合文字列は記号コマンドと同じ [`HirMathKind::Symbol`] にし（字は書いたまま、クラスは記号表から取る）、
/// そのほかの字の並びは 1 個の [`HirMathKind::Text`] にまとめる。
fn push_math_text(ctx: &EvalContext<'_>, text: &str, start: u32, nodes: &mut Vec<HirMath>) {
  let span = |from: usize, to: usize| {
    let at = |offset: usize| {
      return start
        + u32::try_from(offset).expect("トークンはソースの部分文字列で、ソースは u32::MAX バイトを超えない前提");
    };
    return Span::new(at(from), at(to));
  };
  let mut plain = 0;
  for (from, to) in combining_sequences(text) {
    let Some(class) = direct_symbol_class(&text[from..to]) else {
      continue;
    };
    if plain < from {
      nodes.push(ctx.leaf_math(span(plain, from), HirMathKind::Text(text[plain..from].to_string())));
    }
    nodes.push(ctx.leaf_math(
      span(from, to),
      HirMathKind::Symbol {
        text: text[from..to].to_string(),
        class,
      },
    ));
    plain = to;
  }
  if plain < text.len() {
    nodes.push(ctx.leaf_math(span(plain, text.len()), HirMathKind::Text(text[plain..].to_string())));
  }
  return;
}

/// テキストを結合文字列（基底の字と、後ろに続く結合文字）ごとの（開始, 終了）バイト位置に分ける
///
/// 結合文字の判定は lexer と同じ（[`syntax::is_combining`]）。先頭の結合文字は呼び元が拒否済みなので、最初の区間は 0 から始まる。
fn combining_sequences(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
  return text
    .char_indices()
    .filter(|&(_, ch)| return !syntax::is_combining(ch))
    .map(|(at, _)| return at)
    .chain(iter::once(text.len()))
    .tuple_windows();
}

/// 数式に直接書いた結合文字列 `sequence` が記号表の字ならそのクラス
///
/// NFC にした最初の字で引くので、正準等価な綴りは同じクラスになる（`=` + U+0338 は ≠ の Rel）。NFC で合成されない字
/// （U+2ADC）は分解の基底で引くことになるが、記号表の否定形は基底と同じクラスを持つ（テストが記号表の全字で保つ）。
fn direct_symbol_class(sequence: &str) -> Option<MathClass> {
  return ComposingNormalizerBorrowed::new_nfc()
    .normalize(sequence)
    .chars()
    .next()
    .and_then(symbol::direct_input_class);
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
/// 数式の語彙（字形コマンド・`\frac`・`\sqrt`・伸縮括弧・アクセント・上下線・明示アキ・演算子名・文章・記号表）を名前から 1 回だけ引いた結果。必須引数の個数は
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
  /// 明示アキ（`\thinspace` 等）— 必須引数なし
  Space(MathSpace),
  /// 演算子名（`\lim` 等）— 必須引数なし
  OperatorName {
    /// 組む字（[`OPERATOR_NAMES`] の 2 列目）
    text: &'static str,
    /// display 段で付くスクリプトを上下に積むか
    limits: bool,
  },
  /// 文章（`\text`）— 必須引数 1 個（テキストとして読む文章）
  Prose,
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
      "text" => Some(Self::Prose),
      _ => fence_delimiters(name)
        .map(|(open, close)| return Self::Fenced { open, close })
        .or_else(|| return accent_mark(name).map(|mark| return Self::Accent { mark, wide: false }))
        .or_else(|| return wide_accent_mark(name).map(|mark| return Self::Accent { mark, wide: true }))
        .or_else(|| return bar_side(name).map(|over| return Self::Bar { over }))
        .or_else(|| return math_space(name).map(Self::Space))
        .or_else(|| return operator_name(name).map(|(text, limits)| return Self::OperatorName { text, limits }))
        .or_else(|| return symbol::lookup(name).map(Self::Symbol)),
    };
  }

  /// 必須引数の個数
  fn arg_count(self) -> usize {
    return match self {
      Self::Styled(_) | Self::Sqrt | Self::Fenced { .. } | Self::Accent { .. } | Self::Bar { .. } | Self::Prose => 1,
      Self::Frac => 2,
      Self::Space(_) | Self::OperatorName { .. } | Self::Symbol(_) => 0,
    };
  }

  /// 必須引数の読み取り方を位置順に返す（宣言の無い位置は外側文脈の継承）
  fn arg_modes(self) -> &'static [ArgMode] {
    return match self {
      Self::Prose => &[ArgMode::Text],
      Self::Styled(_)
      | Self::Frac
      | Self::Sqrt
      | Self::Fenced { .. }
      | Self::Accent { .. }
      | Self::Bar { .. }
      | Self::Space(_)
      | Self::OperatorName { .. }
      | Self::Symbol(_) => &[],
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

/// アクセントの基底を OpenType `dtls` のドット無しの字形で組むかの上書き（`\hat[dotless=false]{i}`）
const DOTLESS: OptKey<bool> = opt_args::boolean("dotless");

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

/// 明示アキのコマンド名から、アキの種類を引く（明示アキでなければ `None`）
fn math_space(name: &str) -> Option<MathSpace> {
  return match name {
    "thinspace" => Some(MathSpace::Thin),
    "medspace" => Some(MathSpace::Medium),
    "thickspace" => Some(MathSpace::Thick),
    "quad" => Some(MathSpace::Quad),
    "negthinspace" => Some(MathSpace::NegativeThin),
    _ => None,
  };
}

/// 演算子名の表（コマンド名, 組む字, display 段で付くスクリプトを上下に積むか）
///
/// LaTeX カーネルの演算子名と同じ 32 個で、上下に積むかも LaTeX と同じ（`\nolimits` の付かない 10 個）。組む字は ASCII 英字の語を
/// 半角空白 1 個で区切った列で、空白は語の間の細アキ（LaTeX の `lim\,sup`）。
const OPERATOR_NAMES: &[(&str, &str, bool)] = &[
  ("arccos", "arccos", false),
  ("arcsin", "arcsin", false),
  ("arctan", "arctan", false),
  ("arg", "arg", false),
  ("cos", "cos", false),
  ("cosh", "cosh", false),
  ("cot", "cot", false),
  ("coth", "coth", false),
  ("csc", "csc", false),
  ("deg", "deg", false),
  ("det", "det", true),
  ("dim", "dim", false),
  ("exp", "exp", false),
  ("gcd", "gcd", true),
  ("hom", "hom", false),
  ("inf", "inf", true),
  ("ker", "ker", false),
  ("lg", "lg", false),
  ("lim", "lim", true),
  ("liminf", "lim inf", true),
  ("limsup", "lim sup", true),
  ("ln", "ln", false),
  ("log", "log", false),
  ("max", "max", true),
  ("min", "min", true),
  ("Pr", "Pr", true),
  ("sec", "sec", false),
  ("sin", "sin", false),
  ("sinh", "sinh", false),
  ("sup", "sup", true),
  ("tan", "tan", false),
  ("tanh", "tanh", false),
];

/// 演算子名のコマンド名から、組む字と上下に積むかを引く（演算子名でなければ `None`）
fn operator_name(name: &str) -> Option<(&'static str, bool)> {
  return OPERATOR_NAMES
    .iter()
    .find(|&&(command, _, _)| return command == name)
    .map(|&(_, text, limits)| return (text, limits));
}

/// 数式内のコマンド名から必須引数の個数を引く
///
/// 数式の語彙に無いコマンドは `None` — 評価器が未知のコマンドとして拒否するだけなので個数が定まらず、パーサーは個数で
/// 打ち切らない（レジストリの verbatim 宣言どおりに引数を読み、`$\code{a // b}$` を「未知のコマンド」で
/// 診断できるようにするため）。
pub(super) fn lookup_math_arg_count(name: &str) -> Option<usize> {
  return MathCommandKind::from_name(name).map(MathCommandKind::arg_count);
}

/// 数式内のコマンド名から必須引数の読み取り方を位置順に引く（数式の語彙に無いコマンドと宣言の無いコマンドは空）
pub(super) fn lookup_math_arg_modes(name: &str) -> &'static [ArgMode] {
  return match MathCommandKind::from_name(name) {
    Some(kind) => kind.arg_modes(),
    None => &[],
  };
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
      let dotless = if wide {
        opt_args::no_command_opt_args(&view)?;
        None
      } else {
        opt_args::collect_command_opt_args(&view, &[DOTLESS.decl()])?.get(DOTLESS)
      };
      let base_arg = arity::exactly_one_arg(&view, "1 個（アクセントを付ける数式）")?;
      let id = ctx.alloc(view.span());
      let base = Box::new(math_arg_to_node(source, ctx, base_arg)?);
      return Ok(HirMath::new(
        id,
        HirMathKind::Accent {
          accent: mark,
          wide,
          dotless,
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
    MathCommandKind::Space(space) => {
      opt_args::no_command_opt_args(&view)?;
      arity::no_command_args(&view)?;
      return Ok(ctx.leaf_math(view.span(), HirMathKind::Space(space)));
    },
    MathCommandKind::OperatorName { text, limits } => {
      opt_args::no_command_opt_args(&view)?;
      arity::no_command_args(&view)?;
      return Ok(ctx.leaf_math(
        view.span(),
        HirMathKind::OperatorName {
          text: text.to_string(),
          limits,
        },
      ));
    },
    MathCommandKind::Prose => {
      opt_args::no_command_opt_args(&view)?;
      let arg = arity::exactly_one_arg(&view, "1 個（文章）")?;
      let text = prose_text(source, arg)?;
      return Ok(ctx.leaf_math(view.span(), HirMathKind::Prose(text)));
    },
    MathCommandKind::Symbol(symbol) => {
      opt_args::no_command_opt_args(&view)?;
      arity::no_command_args(&view)?;
      return Ok(ctx.leaf_math(
        view.span(),
        HirMathKind::Symbol {
          text: symbol.ch.to_string(),
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

/// `\text` の引数を文章の文字列にする
///
/// 引数は parser がテキストとして読んだ（`ArgMode::Text`）ので、トークンの読みは本文と同じ（[`inline::inline_from_token`]）。
/// 空白・改行の連なりは語間の空白 1 個にまとめ、間のコメントは読み飛ばす。引数の先頭・末尾の空白も 1 個にして残す。
fn prose_text(source: &str, arg: &CstNode<'_>) -> Result<String, EvalError> {
  let mut text = String::new();
  let mut pending_space = false;
  for child in arg.children {
    let piece = match child {
      CstElement::Token(token) if matches!(token.kind, TokenKind::Whitespace | TokenKind::Newline) => {
        pending_space = true;
        continue;
      },
      CstElement::Token(token) => match inline::inline_from_token(source, token) {
        Some(TokenInline::MergeableText(piece)) => piece.to_string(),
        Some(TokenInline::Leaf(HirInlineKind::Text(piece))) => piece,
        Some(TokenInline::Leaf(HirInlineKind::LineBreak)) => {
          return Err(EvalError::UnsupportedInMathText {
            what: r"\\（強制改行）".to_string(),
            span: token.span.into(),
          });
        },
        Some(TokenInline::Leaf(_)) => {
          unreachable!("inline_from_token がトークンから作る葉は Text と LineBreak だけ")
        },
        Some(TokenInline::ParagraphBreak) => {
          return Err(EvalError::ParagraphBreakInArgument {
            span: token.span.into(),
          });
        },
        None => continue,
      },
      CstElement::Node(node) => match node.kind {
        SyntaxKind::CommandCall => prose_char(source, node)?.to_string(),
        SyntaxKind::InlineMath => {
          return Err(EvalError::UnsupportedInMathText {
            what: "インライン数式 $...$".to_string(),
            span: node.span.into(),
          });
        },
        SyntaxKind::Environment => {
          return Err(EvalError::UnsupportedInMathText {
            what: format!("環境 {}", EnvironmentView::new(node, source).name()),
            span: node.span.into(),
          });
        },
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::MathGroup
        | SyntaxKind::MathSubscript
        | SyntaxKind::MathSuperscript => {
          unreachable!(
            "テキストとして読んだ引数の直下に parser が置くノードは、コマンド呼び出し・環境・インライン数式だけ"
          )
        },
      },
    };
    if pending_space {
      text.push(' ');
      pending_space = false;
    }
    text.push_str(&piece);
  }
  if pending_space {
    text.push(' ');
  }
  return Ok(text);
}

/// `\text` の引数の中のコマンドを字にする（受け付けるのは記号コマンドだけ）
fn prose_char(source: &str, node: &CstNode<'_>) -> Result<char, EvalError> {
  let view = CommandView::new(node, source);
  if let Some(symbol) = symbol::lookup(view.name()) {
    opt_args::no_command_opt_args(&view)?;
    arity::no_command_args(&view)?;
    return Ok(symbol.ch);
  }
  if command::is_registered(view.name()) || MathCommandKind::from_name(view.name()).is_some() {
    return Err(EvalError::UnsupportedInMathText {
      what: format!("\\{}", view.name()),
      span: view.span().into(),
    });
  }
  return Err(EvalError::UnknownCommand {
    name: view.name().to_string(),
    span: view.span().into(),
  });
}

#[cfg(test)]
mod tests {
  use super::{MathCommandKind, OPERATOR_NAMES, direct_symbol_class, symbol};

  #[test]
  fn every_symbol_name_resolves_to_the_symbol_in_math() {
    // 数式では字体・frac・sqrt・伸縮括弧・アクセント・上下線を記号表より先に引くので、同名の記号は数式でだけ隠れる
    for name in symbol::names() {
      assert!(
        matches!(MathCommandKind::from_name(name), Some(MathCommandKind::Symbol(_))),
        "数式の語彙が記号名を隠しています: {name}"
      );
    }
  }

  #[test]
  fn every_operator_name_resolves_to_its_operator_in_math() {
    for &(name, text, limits) in OPERATOR_NAMES {
      assert!(
        matches!(
          MathCommandKind::from_name(name),
          Some(MathCommandKind::OperatorName { text: t, limits: l }) if t == text && l == limits
        ),
        "数式の語彙が演算子名を隠しているか、表と違う値を返しています: {name}"
      );
    }
  }

  #[test]
  fn operator_names_are_the_thirty_two_latex_log_like_names() {
    let limits: Vec<&str> = OPERATOR_NAMES.iter().filter(|entry| return entry.2).map(|entry| return entry.0).collect();
    let shoulders: Vec<&str> =
      OPERATOR_NAMES.iter().filter(|entry| return !entry.2).map(|entry| return entry.0).collect();

    assert_eq!(
      limits,
      [
        "det", "gcd", "inf", "lim", "liminf", "limsup", "max", "min", "Pr", "sup"
      ]
    );
    assert_eq!(
      shoulders,
      [
        "arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "dim", "exp", "hom", "ker",
        "lg", "ln", "log", "sec", "sin", "sinh", "tan", "tanh"
      ]
    );
  }

  #[test]
  fn operator_name_texts_are_ascii_words_separated_by_single_spaces() {
    // lowering は空白 1 個を語の区切り（細アキ）として split する
    for &(name, text, _) in OPERATOR_NAMES {
      assert!(
        text
          .split(' ')
          .all(|word| return !word.is_empty() && word.chars().all(|ch| return ch.is_ascii_alphabetic())),
        "{name}: {text:?}"
      );
    }
  }

  #[test]
  fn direct_input_of_each_table_character_takes_its_own_class() {
    // クラスは NFC にした最初の字で引くが、記号表の字そのものを書けば（NFC で合成されない U+2ADC も）その字のクラス
    for name in symbol::names() {
      let ch = symbol::lookup(name).expect("記号表の名前は引ける").ch;

      assert_eq!(direct_symbol_class(&ch.to_string()), symbol::direct_input_class(ch), "U+{:04X}", u32::from(ch));
    }
  }
}
