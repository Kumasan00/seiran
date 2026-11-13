//! 数式モードの評価
//!
//! インライン数式と数式環境のセルを [`HirMath`] 列に変換する。
//!
//! コマンドの数式引数（[`math_arg_to_node`]）のように単一ノードへ畳まれて、先に確保したグループ用の ID が使われない場合は
//! `NodeId` に穴が空くが、同じ入力なら常に同じ穴になるので決定性は保たれる。

use std::iter;

use icu_normalizer::ComposingNormalizerBorrowed;
use itertools::Itertools;
use phf::phf_map;

use crate::{
  document::{HirInlineKind, HirMath, HirMathKind, MathClass, MathSpace, MathVariant, NodeId},
  frontend::{
    evaluator::{
      self, EvalContext, EvalError, command,
      command::symbol::{self, MathSymbol},
      environment,
      inline::{self, TokenInline},
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::{Bound, Signature},
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
pub(super) fn evaluate_math_elements(
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
          nodes.push(environment::evaluate_in_math(&EnvironmentView::new(child_node, source), ctx)?);
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
  return Some(Span::new(token.span.start, evaluator::source_position(token.span.start, len)));
}

/// 数式のテキストトークンの中身 `text`（ソース上の開始位置 `start`）を HIR へ積む
///
/// 記号表の字の結合文字列は記号コマンドと同じ [`HirMathKind::Symbol`] にし（字は書いたまま、クラスは記号表から取る）、
/// そのほかの字の並びは 1 個の [`HirMathKind::Text`] にまとめる。
fn push_math_text(ctx: &EvalContext<'_>, text: &str, start: u32, nodes: &mut Vec<HirMath>) {
  let span = |from: usize, to: usize| {
    let at = |offset: usize| return evaluator::source_position(start, offset);
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
  return match <[HirMath; 1]>::try_from(nodes) {
    Ok([single]) => single,
    Err(nodes) => HirMath::new(group_id, HirMathKind::Group(nodes)),
  };
}

/// 数式内コマンドの種類（必須引数の個数で 3 つに分ける。個数はこの分け方が型で決める）
///
/// 数式の語彙（字形コマンド・`\frac`・`\binom`・`\sqrt`・伸縮括弧・アクセント・上下線・明示アキ・演算子名・文章・中間の区切り・
/// 記号表）を名前から 1 回だけ引いた結果。パーサーが数式内で引数を読む上限（[`lookup_math_arg_count`]）も、評価の個数検査
/// （[`evaluate_math_command`] の外側 3 arm の `bind`）も、この分け方が決める個数に従う。
#[derive(Debug, Clone, Copy)]
enum MathCommandKind {
  /// 必須引数を取らない数式コマンド
  Leaf(MathLeaf),
  /// 必須引数を 1 個取る数式コマンド
  Unary(MathUnary),
  /// 必須引数を 2 個取る数式コマンド
  Binary(MathBinary),
}

/// 必須引数を取らない数式コマンド
#[derive(Debug, Clone, Copy)]
enum MathLeaf {
  /// 明示アキ（`\thinspace` 等）
  Space(MathSpace),
  /// 演算子名（`\lim` 等）
  OperatorName {
    /// 組む字（ASCII 英字の語を半角空白 1 個で区切った列。空白は語の間の細アキ）
    text: &'static str,
    /// display 段で付くスクリプトを上下に積むか
    limits: bool,
  },
  /// 中間の区切り（`\given`）。伸縮括弧の引数の直下でだけ有効
  Given,
  /// 記号コマンド（`\alpha` 等）
  Symbol(MathSymbol),
}

/// 必須引数を 1 個取る数式コマンド
#[derive(Debug, Clone, Copy)]
enum MathUnary {
  /// 字形コマンド（`\mathbold` 等）
  Styled(MathVariant),
  /// `\sqrt` — 根指数は任意引数で、数式内容として読む
  Sqrt,
  /// 伸縮括弧（`\paren` 等）
  Fenced {
    /// 左の区切り括弧
    open: char,
    /// 右の区切り括弧
    close: char,
  },
  /// アクセント（`\hat` 等・`\widehat` 等）
  Accent {
    /// 基底の上に置く結合用ダイアクリティカルマーク
    mark: char,
    /// 記号を基底の送り幅へ横に伸ばすか（広幅アクセント）
    wide: bool,
  },
  /// 上線・下線（`\overline` / `\underline`）
  Bar {
    /// 線を基底の上に引くか（上線は `true`、下線は `false`）
    over: bool,
  },
  /// 文章（`\text`）— 引数はテキストとして読む
  Prose,
  /// 数値（`\num`）
  Number,
  /// 単位（`\unit`）
  Unit,
}

/// 必須引数を 2 個取る数式コマンド
#[derive(Debug, Clone, Copy)]
enum MathBinary {
  /// `\frac`（分子と分母）
  Frac,
  /// `\binom`（上段と下段）
  Binom,
  /// 量（`\qty`。数値と単位の式）
  Quantity,
}

/// アクセントの基底を OpenType `dtls` のドット無しの字形で組むかの上書き（`\hat[dotless=false]{i}`）
const DOTLESS: OptKey<bool> = opt_args::boolean("dotless");
/// アクセント（広幅を除く）が受け付けるキー
const ACCENT_KEYS: &[OptDecl] = &[DOTLESS.decl()];

/// 記号を除く数式の語彙（名前 → 種別）
///
/// 記号は本文と共有する記号表（`symbol::lookup`）が引く。同じ名前が両方にあれば数式ではこちらが勝つ。
static MATH_COMMANDS: phf::Map<&'static str, MathCommandKind> = phf_map! {
  // 字形コマンド
  "mathserif" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Serif)),
  "mathitalic" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Italic)),
  "mathbold" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Bold)),
  "mathbolditalic" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::BoldItalic)),
  "mathsans" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Sans)),
  "mathsansitalic" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::SansItalic)),
  "mathsansbold" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::SansBold)),
  "mathsansbolditalic" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::SansBoldItalic)),
  "mathmono" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Mono)),
  "mathdoublestruck" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::DoubleStruck)),
  "mathscript" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Script)),
  "mathcalligraphic" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Calligraphic)),
  "mathfraktur" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::Fraktur)),
  "mathscriptbold" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::ScriptBold)),
  "mathfrakturbold" => MathCommandKind::Unary(MathUnary::Styled(MathVariant::FrakturBold)),

  "frac" => MathCommandKind::Binary(MathBinary::Frac),
  "binom" => MathCommandKind::Binary(MathBinary::Binom),
  "sqrt" => MathCommandKind::Unary(MathUnary::Sqrt),
  "text" => MathCommandKind::Unary(MathUnary::Prose),
  "num" => MathCommandKind::Unary(MathUnary::Number),
  "unit" => MathCommandKind::Unary(MathUnary::Unit),
  "qty" => MathCommandKind::Binary(MathBinary::Quantity),
  "given" => MathCommandKind::Leaf(MathLeaf::Given),

  // 伸縮括弧。`\abs` は U+007C、`\norm` は U+2016。`\anglebracket` は `\langle` / `\rangle` と同じ U+27E8 / U+27E9
  // （U+2329 / U+232A は NFC で CJK の U+3008 / U+3009 に写る）
  "paren" => MathCommandKind::Unary(MathUnary::Fenced { open: '(', close: ')' }),
  "bracket" => MathCommandKind::Unary(MathUnary::Fenced { open: '[', close: ']' }),
  "brace" => MathCommandKind::Unary(MathUnary::Fenced { open: '{', close: '}' }),
  "abs" => MathCommandKind::Unary(MathUnary::Fenced { open: '|', close: '|' }),
  "norm" => MathCommandKind::Unary(MathUnary::Fenced { open: '\u{2016}', close: '\u{2016}' }),
  "anglebracket" => MathCommandKind::Unary(MathUnary::Fenced { open: '\u{27E8}', close: '\u{27E9}' }),
  "floor" => MathCommandKind::Unary(MathUnary::Fenced { open: '\u{230A}', close: '\u{230B}' }),
  "ceil" => MathCommandKind::Unary(MathUnary::Fenced { open: '\u{2308}', close: '\u{2309}' }),

  // アクセント。対応は unicode-math の `\mathaccent`（`unicode-math-table.tex`）と同じ。数式フォントの OpenType MATH は
  // 結合記号の側に `MathTopAccentAttachment` と `flac` の字形を持つ（スペーシング形の U+02C6 等は持たない）
  "hat" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0302}', wide: false }),
  "bar" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0304}', wide: false }),
  "vec" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{20D7}', wide: false }),
  "dot" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0307}', wide: false }),
  "ddot" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0308}', wide: false }),
  "tilde" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0303}', wide: false }),
  "check" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{030C}', wide: false }),
  "acute" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0301}', wide: false }),
  "grave" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0300}', wide: false }),
  "breve" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0306}', wide: false }),

  // 広幅アクセント。`\widehat` / `\widetilde` / `\widecheck` は unicode-math の `\mathaccentwide` と同じ字、矢印は結合用の
  // 矢印（上）。横に伸ばす字形は数式フォントの OpenType MATH が結合記号の側に持つ横方向の size variant と glyph assembly
  // で、持たないフォントでは元の字形のまま組む
  "widehat" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0302}', wide: true }),
  "widetilde" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{0303}', wide: true }),
  "widecheck" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{030C}', wide: true }),
  "overrightarrow" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{20D7}', wide: true }),
  "overleftarrow" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{20D6}', wide: true }),
  "overleftrightarrow" => MathCommandKind::Unary(MathUnary::Accent { mark: '\u{20E1}', wide: true }),

  "overline" => MathCommandKind::Unary(MathUnary::Bar { over: true }),
  "underline" => MathCommandKind::Unary(MathUnary::Bar { over: false }),

  "thinspace" => MathCommandKind::Leaf(MathLeaf::Space(MathSpace::Thin)),
  "medspace" => MathCommandKind::Leaf(MathLeaf::Space(MathSpace::Medium)),
  "thickspace" => MathCommandKind::Leaf(MathLeaf::Space(MathSpace::Thick)),
  "quad" => MathCommandKind::Leaf(MathLeaf::Space(MathSpace::Quad)),
  "negthinspace" => MathCommandKind::Leaf(MathLeaf::Space(MathSpace::NegativeThin)),

  // 演算子名。LaTeX カーネルの演算子名と同じ 32 個で、上下に積むかも LaTeX と同じ（`\nolimits` の付かない 10 個）。
  // 組む字の空白は語の間の細アキ（LaTeX の `lim\,sup`）
  "arccos" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "arccos", limits: false }),
  "arcsin" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "arcsin", limits: false }),
  "arctan" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "arctan", limits: false }),
  "arg" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "arg", limits: false }),
  "cos" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "cos", limits: false }),
  "cosh" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "cosh", limits: false }),
  "cot" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "cot", limits: false }),
  "coth" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "coth", limits: false }),
  "csc" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "csc", limits: false }),
  "deg" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "deg", limits: false }),
  "det" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "det", limits: true }),
  "dim" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "dim", limits: false }),
  "exp" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "exp", limits: false }),
  "gcd" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "gcd", limits: true }),
  "hom" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "hom", limits: false }),
  "inf" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "inf", limits: true }),
  "ker" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "ker", limits: false }),
  "lg" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "lg", limits: false }),
  "lim" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "lim", limits: true }),
  "liminf" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "lim inf", limits: true }),
  "limsup" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "lim sup", limits: true }),
  "ln" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "ln", limits: false }),
  "log" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "log", limits: false }),
  "max" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "max", limits: true }),
  "min" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "min", limits: true }),
  "Pr" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "Pr", limits: true }),
  "sec" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "sec", limits: false }),
  "sin" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "sin", limits: false }),
  "sinh" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "sinh", limits: false }),
  "sup" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "sup", limits: true }),
  "tan" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "tan", limits: false }),
  "tanh" => MathCommandKind::Leaf(MathLeaf::OperatorName { text: "tanh", limits: false }),
};

impl MathCommandKind {
  /// コマンド名から数式の語彙を引く。数式の語彙に無ければ `None`
  fn from_name(name: &str) -> Option<Self> {
    return MATH_COMMANDS
      .get(name)
      .copied()
      .or_else(|| return symbol::lookup(name).map(|symbol| return Self::Leaf(MathLeaf::Symbol(symbol))));
  }

  /// 必須引数の個数（外側の variant が決める）
  fn arg_count(self) -> usize {
    return match self {
      Self::Leaf(_) => 0,
      Self::Unary(_) => 1,
      Self::Binary(_) => 2,
    };
  }

  /// 必須引数の位置 `index`（0 始まり）の読み取り方（宣言の範囲外は外側文脈の継承）
  fn arg_mode(self, index: usize) -> ArgMode {
    return match self {
      Self::Leaf(command) => command.signature().arg_mode(index),
      Self::Unary(command) => command.signature().arg_mode(index),
      Self::Binary(command) => command.signature().arg_mode(index),
    };
  }
}

impl MathLeaf {
  /// 署名（必須引数も任意引数も取らない）
  fn signature(self) -> Signature<0> {
    return match self {
      Self::Space(_) | Self::OperatorName { .. } | Self::Given | Self::Symbol(_) => Signature::NONE,
    };
  }

  /// 検査済みの数式コマンドを評価する
  fn evaluate(self, view: &CommandView<'_>, ctx: &EvalContext<'_>) -> Result<HirMath, EvalError> {
    return match self {
      Self::Space(space) => Ok(ctx.leaf_math(view.span(), HirMathKind::Space(space))),
      Self::OperatorName { text, limits } => Ok(ctx.leaf_math(
        view.span(),
        HirMathKind::OperatorName {
          text: text.to_string(),
          limits,
        },
      )),
      // 伸縮括弧の直下の `\given` は `evaluate_fence_segments` が区切りとして取り除くので、ここへ来るのはそれ以外の位置
      // （グループ・引数・スクリプト・セル・環境本体・伸縮括弧の外）の `\given` だけ
      Self::Given => Err(EvalError::GivenNotInFence {
        span: view.span().into(),
      }),
      Self::Symbol(symbol) => Ok(ctx.leaf_math(
        view.span(),
        HirMathKind::Symbol {
          text: symbol.ch.to_string(),
          class: symbol.class,
        },
      )),
    };
  }
}

/// 字形・伸縮括弧・上下線・広幅アクセントの署名（任意引数を取らないので不足時の説明だけが違う）
const fn plain_unary(expected: &'static str) -> Signature<1> {
  return Signature::new([ArgMode::Inherit], expected, &[]);
}

impl MathUnary {
  /// 署名
  fn signature(self) -> Signature<1> {
    return match self {
      Self::Styled(_) => plain_unary("1 個（数式本体）"),
      Self::Sqrt => Signature::math_index([ArgMode::Inherit], "1 個（被開平数）"),
      Self::Fenced { .. } => plain_unary("1 個（括弧で包む数式）"),
      Self::Accent { wide: false, .. } => {
        Signature::new([ArgMode::Inherit], "1 個（アクセントを付ける数式）", ACCENT_KEYS)
      },
      Self::Accent { wide: true, .. } => plain_unary("1 個（アクセントを付ける数式）"),
      Self::Bar { .. } => plain_unary("1 個（線を引く数式）"),
      Self::Prose => Signature::new([ArgMode::Text], "1 個（文章）", &[]),
      Self::Number => command::number::SIGNATURE,
      Self::Unit => command::unit::SIGNATURE,
    };
  }

  /// 検査済みの引数 `arg` と任意引数 `opts` で数式コマンドを評価する
  fn evaluate(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    arg: &CstNode<'_>,
    opts: &OptArgs,
  ) -> Result<HirMath, EvalError> {
    let source = view.source();
    match self {
      Self::Styled(variant) => {
        let id = ctx.alloc(view.span());
        let children = evaluate_math_children(source, ctx, arg)?;
        return Ok(HirMath::new(id, HirMathKind::Styled { variant, children }));
      },
      Self::Sqrt => {
        // 個数は外側の `bind` が根指数の評価より前に検査済み — `math_arg_to_node` は `ctx.alloc` で NodeId を消費する
        // ので、引数の個数が誤っていて拒否するだけの入力に対して、その割り当てを発生させない
        let id = ctx.alloc(view.span());
        let index = match view.opt_arg() {
          Some(opt) => Some(Box::new(math_arg_to_node(source, ctx, opt)?)),
          None => None,
        };
        let radicand = Box::new(math_arg_to_node(source, ctx, arg)?);
        return Ok(HirMath::new(id, HirMathKind::Sqrt { index, radicand }));
      },
      Self::Fenced { open, close } => {
        let id = ctx.alloc(view.span());
        let segments = evaluate_fence_segments(source, ctx, arg)?;
        return Ok(HirMath::new(
          id,
          HirMathKind::Fenced {
            open,
            close,
            segments,
          },
        ));
      },
      Self::Accent { mark, wide } => {
        let id = ctx.alloc(view.span());
        let base = Box::new(math_arg_to_node(source, ctx, arg)?);
        return Ok(HirMath::new(
          id,
          HirMathKind::Accent {
            accent: mark,
            wide,
            dotless: opts.get(DOTLESS),
            base,
          },
        ));
      },
      Self::Bar { over } => {
        let id = ctx.alloc(view.span());
        let body = Box::new(math_arg_to_node(source, ctx, arg)?);
        return Ok(HirMath::new(id, HirMathKind::Bar { over, body }));
      },
      Self::Prose => {
        let text = prose_text(source, arg)?;
        return Ok(ctx.leaf_math(view.span(), HirMathKind::Prose(text)));
      },
      Self::Number => {
        let numeral = command::number::numeral_arg(source, arg)?;
        return Ok(ctx.leaf_math(view.span(), HirMathKind::Number(numeral)));
      },
      Self::Unit => {
        let factors = command::unit::unit_factors_arg(source, arg)?;
        return Ok(ctx.leaf_math(view.span(), HirMathKind::Unit(factors)));
      },
    }
  }
}

impl MathBinary {
  /// 署名
  fn signature(self) -> Signature<2> {
    return match self {
      Self::Frac => Signature::new([ArgMode::Inherit, ArgMode::Inherit], "2 個（分子と分母）", &[]),
      Self::Binom => Signature::new([ArgMode::Inherit, ArgMode::Inherit], "2 個（上段と下段）", &[]),
      Self::Quantity => command::quantity::SIGNATURE,
    };
  }

  /// 検査済みの引数 `args` で数式コマンドを評価する（2 引数の数式コマンドはどれも任意引数を取らない）
  fn evaluate(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    [first, second]: [&CstNode<'_>; 2],
  ) -> Result<HirMath, EvalError> {
    let source = view.source();
    match self {
      Self::Frac => {
        let id = ctx.alloc(view.span());
        let numer = Box::new(math_arg_to_node(source, ctx, first)?);
        let denom = Box::new(math_arg_to_node(source, ctx, second)?);
        return Ok(HirMath::new(id, HirMathKind::Frac { numer, denom }));
      },
      Self::Binom => {
        let id = ctx.alloc(view.span());
        let top = Box::new(math_arg_to_node(source, ctx, first)?);
        let bottom = Box::new(math_arg_to_node(source, ctx, second)?);
        return Ok(HirMath::new(id, HirMathKind::Binom { top, bottom }));
      },
      Self::Quantity => {
        let (numeral, unit) = command::quantity::quantity(source, [first, second])?;
        return Ok(ctx.leaf_math(view.span(), HirMathKind::Quantity { numeral, unit }));
      },
    }
  }
}

/// 数式内のコマンド名から必須引数の個数を引く
///
/// 数式の語彙に無いコマンドは `None` — 評価器が未知のコマンドとして拒否するだけなので個数が定まらず、パーサーは個数で
/// 打ち切らない（レジストリの verbatim 宣言どおりに引数を読み、`$\code{a // b}$` を「未知のコマンド」で
/// 診断できるようにするため）。
pub(super) fn lookup_math_arg_count(name: &str) -> Option<usize> {
  return MathCommandKind::from_name(name).map(MathCommandKind::arg_count);
}

/// 数式内のコマンド名と必須引数の位置（0 始まり）から読み取り方を引く（数式の語彙に無いコマンドと宣言の範囲外は継承）
pub(super) fn lookup_math_arg_mode(name: &str, index: usize) -> ArgMode {
  return MathCommandKind::from_name(name).map_or(ArgMode::Inherit, |kind| return kind.arg_mode(index));
}

/// 数式内コマンドを [`HirMath`] に変換する
///
/// 必須引数の個数と未知キーの検査は外側 3 arm の `bind` だけが行う。数式内ではパーサーが署名の個数で引数の読みを
/// 打ち切るので、ここで実際に起きうる個数の誤りは不足だけになる。
fn evaluate_math_command(source: &str, ctx: &EvalContext<'_>, cmd_node: &CstNode<'_>) -> Result<HirMath, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let Some(kind) = MathCommandKind::from_name(view.name()) else {
    return Err(EvalError::UnknownCommand {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  };

  match kind {
    MathCommandKind::Leaf(command) => {
      command.signature().bind(&view)?;
      return command.evaluate(&view, ctx);
    },
    MathCommandKind::Unary(command) => {
      let Bound { args: [arg], opts } = command.signature().bind(&view)?;
      return command.evaluate(&view, ctx, arg, &opts);
    },
    MathCommandKind::Binary(command) => {
      let Bound { args, .. } = command.signature().bind(&view)?;
      return command.evaluate(&view, ctx, args);
    },
  }
}

/// 数式引数ノードを単一の [`HirMath`] に変換するヘルパー
fn math_arg_to_node(source: &str, ctx: &EvalContext<'_>, arg_node: &CstNode<'_>) -> Result<HirMath, EvalError> {
  let group_id = ctx.alloc(arg_node.span);
  let nodes = evaluate_math_children(source, ctx, arg_node)?;
  return Ok(collapse_single(group_id, nodes));
}

/// 伸縮括弧の引数ノード `body` を、直下の `\given` で区切った区画の列（[`HirMathKind::Fenced`] の `segments`）に変換する
fn evaluate_fence_segments(
  source: &str,
  ctx: &EvalContext<'_>,
  body: &CstNode<'_>,
) -> Result<Vec<Vec<HirMath>>, EvalError> {
  let is_given = |element: &CstElement<'_>| {
    return match element {
      CstElement::Node(node) if node.kind == SyntaxKind::CommandCall => {
        matches!(
          MathCommandKind::from_name(CommandView::new(node, source).name()),
          Some(MathCommandKind::Leaf(MathLeaf::Given))
        )
      },
      CstElement::Node(_) | CstElement::Token(_) => false,
    };
  };
  for element in body.children.iter().filter(|element| return is_given(element)) {
    let CstElement::Node(node) = element else {
      unreachable!("`is_given` はコマンド呼び出しのノードだけを選ぶ")
    };
    MathLeaf::Given.signature().bind(&CommandView::new(node, source))?;
  }
  return body
    .children
    .split(is_given)
    .map(|segment| return evaluate_math_elements(source, ctx, segment))
    .collect();
}

/// `\text` の引数を [`HirMathKind::Prose`] の文字列にする
///
/// 引数は parser がテキストとして読んだ（`ArgMode::Text`）ので、トークンの読みは本文と同じ（[`inline::inline_from_token`]）。
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
  if let Some(ch) = command::symbol_char(&view)? {
    return Ok(ch);
  }
  if matches!(MathCommandKind::from_name(view.name()), Some(MathCommandKind::Leaf(MathLeaf::Given))) {
    return Err(EvalError::GivenNotInFence {
      span: view.span().into(),
    });
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
  use bumpalo::Bump;

  use super::{MATH_COMMANDS, MathCommandKind, MathLeaf, direct_symbol_class, symbol};
  use crate::{
    document::HirNode,
    frontend::evaluator::{EvalError, evaluate_children_to_hir, test_support},
  };

  impl MathCommandKind {
    /// 任意引数を名前付きキーとして検査するか（根指数を読む `\sqrt` だけが `false`）
    fn reads_keys(self) -> bool {
      return match self {
        Self::Leaf(command) => command.signature().reads_keys(),
        Self::Unary(command) => command.signature().reads_keys(),
        Self::Binary(command) => command.signature().reads_keys(),
      };
    }
  }

  /// `$\name[opts]{a}…$`（`{a}` を `arg_count` 個）を評価する
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテストの組み立て誤りで評価の結果ではないので、panic させてテストを落とす必要がある"
  )]
  fn evaluate_placeholder_math(name: &str, opts: &str, arg_count: usize) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let source = format!("$\\{name}{opts}{}$", "{a}".repeat(arg_count));
    let cst = test_support::parse_cst(&source, &arena).unwrap();
    return evaluate_children_to_hir(&source, cst);
  }

  /// 演算子名の表（コマンド名, 組む字, 上下に積むか）を名前順に返す
  fn operator_names() -> Vec<(&'static str, &'static str, bool)> {
    let mut names: Vec<(&'static str, &'static str, bool)> = MATH_COMMANDS
      .entries()
      .filter_map(|(name, kind)| {
        return match kind {
          MathCommandKind::Leaf(MathLeaf::OperatorName { text, limits }) => Some((*name, *text, *limits)),
          MathCommandKind::Leaf(_) | MathCommandKind::Unary(_) | MathCommandKind::Binary(_) => None,
        };
      })
      .collect();
    names.sort_unstable();
    return names;
  }

  #[test]
  fn every_math_command_rejects_a_dropped_argument() {
    for (name, kind) in MATH_COMMANDS.entries() {
      let Some(dropped) = kind.arg_count().checked_sub(1) else {
        continue;
      };

      let result = evaluate_placeholder_math(name, "", dropped);

      assert!(
        matches!(result, Err(EvalError::MissingCommandArgument { name: ref rejected, .. }) if rejected == name),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn every_math_command_rejects_an_undeclared_key() {
    // 引数を足す側は parser が個数で打ち切る（後ろの `{...}` は数式グループになる）ので数式の中では書けない
    for (name, kind) in MATH_COMMANDS.entries().filter(|(_, kind)| return kind.reads_keys()) {
      let result = evaluate_placeholder_math(name, "[undeclared=1]", kind.arg_count());

      assert!(
        matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "undeclared"),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn given_in_fence_rejects_an_undeclared_key() {
    let arena = Bump::new();
    let source = r"$\paren{a \given[k=v] b}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"), "{result:?}");
  }

  #[test]
  fn sqrt_index_is_math_content() {
    let arena = Bump::new();
    let with_index = r"$\sqrt[3]{x}$";
    let without_radicand = r"$\sqrt[3]$";
    let with_index_cst = test_support::parse_cst(with_index, &arena).unwrap();
    let without_radicand_cst = test_support::parse_cst(without_radicand, &arena).unwrap();

    let accepted = evaluate_children_to_hir(with_index, with_index_cst);
    let rejected = evaluate_children_to_hir(without_radicand, without_radicand_cst);

    assert!(accepted.is_ok(), "{accepted:?}");
    assert!(
      matches!(rejected, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "sqrt"),
      "{rejected:?}"
    );
  }

  #[test]
  fn every_symbol_name_resolves_to_the_symbol_in_math() {
    // 数式では数式の語彙のレジストリを記号表より先に引くので、同名の記号は数式でだけ隠れる
    for name in symbol::names() {
      assert!(
        matches!(MathCommandKind::from_name(name), Some(MathCommandKind::Leaf(MathLeaf::Symbol(_)))),
        "数式の語彙が記号名を隠しています: {name}"
      );
    }
  }

  #[test]
  fn operator_names_are_the_thirty_two_latex_log_like_names() {
    let names = operator_names();
    let limits: Vec<&str> = names.iter().filter(|entry| return entry.2).map(|entry| return entry.0).collect();
    let shoulders: Vec<&str> = names.iter().filter(|entry| return !entry.2).map(|entry| return entry.0).collect();

    assert_eq!(
      limits,
      [
        "Pr", "det", "gcd", "inf", "lim", "liminf", "limsup", "max", "min", "sup"
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
    for (name, text, _) in operator_names() {
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
