//! `\unit{単位の式}` の評価 — 数式として読んだ引数を単位の文法で因子に分け、語彙で検証して [`UnitFactor`] の列にする
//!
//! 単位の式は因子を `.` で区切った並びで、因子は `[接頭辞]単位[^{指数}]`。`.` は字句のトークンではなく、この文法が
//! 字の列の上に課す区切り。記号コマンドは字に置き換えて読み（直後の空白は parser がコマンド呼び出しの中に入れるので
//! 因子を切らない）、そのほかのコマンド・グループ・下付き・空白は文法から外れたものとして扱う。

mod vocabulary;

use std::mem;

use crate::{
  document::UnitFactor,
  frontend::{
    evaluator::{self, EvalError, arity, command, math, opt_args},
    syntax::{CstElement, CstNode, SyntaxKind, token::TokenKind, view::CommandView},
  },
  source::Span,
};

/// 構文から外れた要素の既定の label
const OUTSIDE_GRAMMAR: &str = "ここで単位の書き方から外れています";

/// `\unit` の引数を単位の文法で読み、語彙で検証した因子の列（空でない、ソース順）を返す
///
/// # Errors
///
/// 任意引数・引数の個数の誤り、文法から外れた引数（最初の 1 件）、語彙で読めない因子を返す。
pub(in crate::frontend::evaluator) fn unit_factors(view: &CommandView<'_>) -> Result<Vec<UnitFactor>, EvalError> {
  opt_args::no_command_opt_args(view)?;
  let arg = arity::exactly_one_arg(view, "1 個（単位）")?;
  let items = arg_items(view.source(), arg)?;
  if items.is_empty() {
    return Err(syntax(arg.span, "単位がありません"));
  }
  let factors = read_factors(view.source(), &items)?;
  for (factor, span) in &factors {
    vocabulary::check(&factor.symbol, *span)?;
  }
  return Ok(factors.into_iter().map(|(factor, _)| return factor).collect());
}

/// 引数の中身を左から読んだ要素
enum Item<'a> {
  /// 字（字のトークンの 1 字・エスケープした字・記号コマンドの字）と、そのソース上の位置
  Char(char, Span),
  /// 上付き（`^{...}`）。直前の因子の指数
  Superscript(&'a CstNode<'a>),
  /// 文法から外れた要素（空白・グループ・下付き・記号以外のコマンドなど）と label
  Other(Span, &'static str),
}

/// 引数ノード `arg` の中身を [`Item`] の列にする（引数を区切る括弧とコメントは除く）
///
/// # Errors
///
/// 記号コマンドに引数・任意引数があるとき、未知のコマンドがあるときのエラーを返す。
fn arg_items<'a>(source: &str, arg: &'a CstNode<'a>) -> Result<Vec<Item<'a>>, EvalError> {
  let mut items = Vec::new();
  for child in arg.children {
    match child {
      CstElement::Token(token) => match token.kind {
        TokenKind::Text => {
          for (offset, ch) in token.text(source).char_indices() {
            let start = evaluator::source_position(token.span.start, offset);
            items.push(Item::Char(ch, char_span(start, ch)));
          }
        },
        // エスケープできる字（構文文字と `/`）はどれも単位の記号に入らない。`\/` は裸の `/` と同じ診断にするため字として読む
        TokenKind::Escaped => items.push(if token.text(source)[1..].starts_with('/') {
          Item::Char('/', token.span)
        } else {
          Item::Other(token.span, OUTSIDE_GRAMMAR)
        }),
        TokenKind::LBrace | TokenKind::RBrace | TokenKind::Comment => {},
        TokenKind::Whitespace | TokenKind::Newline => items.push(Item::Other(token.span, "空白は書けません")),
        TokenKind::VerbatimText
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::Ampersand
        | TokenKind::LineBreak
        | TokenKind::Command
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::Dollar
        | TokenKind::Underscore
        | TokenKind::Caret
        | TokenKind::ParagraphBreak
        | TokenKind::CombiningMarkOnSyntaxChar
        | TokenKind::Unknown => items.push(Item::Other(token.span, OUTSIDE_GRAMMAR)),
      },
      CstElement::Node(node) => items.push(match node.kind {
        SyntaxKind::CommandCall => command_item(source, node)?,
        SyntaxKind::MathSuperscript => Item::Superscript(node),
        _ => Item::Other(node.span, OUTSIDE_GRAMMAR),
      }),
    }
  }
  return Ok(items);
}

/// 引数の中のコマンド呼び出し `node` を要素にする（記号コマンドだけが字になる。`\text` の引数の中と同じ切り分け）
///
/// # Errors
///
/// 記号コマンドに引数・任意引数があるとき、未知のコマンドのときのエラーを返す。
fn command_item<'a>(source: &str, node: &'a CstNode<'a>) -> Result<Item<'a>, EvalError> {
  let view = CommandView::new(node, source);
  if let Some(ch) = command::symbol_char(&view)? {
    let Some(CstElement::Token(name)) = node.children.first() else {
      unreachable!("コマンド呼び出しノードの先頭の子はコマンド名のトークン（parser の parse_command_call）")
    };
    return Ok(Item::Char(ch, name.span));
  }
  if command::is_registered(view.name()) || math::lookup_math_arg_count(view.name()).is_some() {
    return Ok(Item::Other(view.span(), OUTSIDE_GRAMMAR));
  }
  return Err(EvalError::UnknownCommand {
    name: view.name().to_string(),
    span: view.span().into(),
  });
}

/// 要素の列を因子（と、その記号のソース上の範囲）に分ける
///
/// # Errors
///
/// 文法から外れた最初の箇所の `invalid_unit_syntax` か `invalid_unit_exponent` を返す。
fn read_factors(source: &str, items: &[Item<'_>]) -> Result<Vec<(UnitFactor, Span)>, EvalError> {
  let mut factors = Vec::new();
  let mut symbol = String::new();
  let mut symbol_span: Option<Span> = None;
  let mut exponent = None;
  // 直前の区切り `.` の位置（末尾の `.` を指す）
  let mut last_separator = None;
  for item in items {
    match item {
      Item::Char('.', span) => {
        let Some(range) = symbol_span.take() else {
          return Err(syntax(*span, "`.` の前に単位がありません"));
        };
        factors.push((
          UnitFactor {
            symbol: mem::take(&mut symbol),
            exponent: exponent.take(),
          },
          range,
        ));
        last_separator = Some(*span);
      },
      Item::Char('/', span) => return Err(syntax(*span, "`/` は使えません。分母は負の指数で書きます")),
      Item::Char(ch, span) => {
        if exponent.is_some() {
          return Err(syntax(*span, "指数の後には `.` が要ります"));
        }
        symbol.push(*ch);
        symbol_span = Some(symbol_span.map_or(*span, |range| return Span::new(range.start, span.end)));
      },
      Item::Superscript(node) => {
        let caret = node.span;
        let caret = Span::new(caret.start, caret.start + 1);
        if symbol_span.is_none() {
          return Err(syntax(caret, "`^` の前に単位がありません"));
        }
        if exponent.is_some() {
          return Err(syntax(caret, "指数は 1 つの単位に 1 回までです"));
        }
        exponent = Some(read_exponent(source, node)?);
      },
      Item::Other(span, label) => return Err(syntax(*span, label)),
    }
  }
  let Some(range) = symbol_span else {
    let at = last_separator.expect("要素の列は空でなく、区切り以外の要素はどれも記号を足すか呼び元へ誤りを返す");
    return Err(syntax(at, "`.` の後に単位がありません"));
  };
  factors.push((UnitFactor { symbol, exponent }, range));
  return Ok(factors);
}

/// 上付きノード `node`（`^` とトリビアと `{...}` のグループ）から指数の数字列を読む
///
/// # Errors
///
/// `^` とグループの間・グループの中の空白は `invalid_unit_syntax`、そのほかの形の誤りは `invalid_unit_exponent`。
fn read_exponent(source: &str, node: &CstNode<'_>) -> Result<String, EvalError> {
  let mut group = None;
  for child in node.children {
    match child {
      CstElement::Token(token) if matches!(token.kind, TokenKind::Whitespace | TokenKind::Newline) => {
        return Err(syntax(token.span, "空白は書けません"));
      },
      CstElement::Token(_) => {},
      CstElement::Node(inner) => group = Some(*inner),
    }
  }
  let group = group.expect("parser の parse_math_script は上付きの中身を `{...}` のグループ 1 個に限る");
  let invalid = || {
    return EvalError::InvalidUnitExponent {
      span: group.span.into(),
    };
  };
  let mut digits = String::new();
  for child in group.children {
    match child {
      CstElement::Token(token) => match token.kind {
        TokenKind::Text => digits.push_str(token.text(source)),
        TokenKind::LBrace | TokenKind::RBrace | TokenKind::Comment => {},
        TokenKind::Whitespace | TokenKind::Newline => return Err(syntax(token.span, "空白は書けません")),
        _ => return Err(invalid()),
      },
      CstElement::Node(_) => return Err(invalid()),
    }
  }
  let magnitude = digits.strip_prefix('-').unwrap_or(&digits);
  let well_formed = magnitude.starts_with(|ch: char| return matches!(ch, '1'..='9'))
    && magnitude.chars().all(|ch| return ch.is_ascii_digit())
    && digits != "1";
  if !well_formed {
    return Err(invalid());
  }
  return Ok(digits);
}

/// 位置 `start` から始まる 1 字 `ch` のソース上の範囲
fn char_span(start: u32, ch: char) -> Span {
  let len = u32::try_from(ch.len_utf8()).expect("1 字の UTF-8 長は 4 以下");
  return Span::new(start, start + len);
}

/// 位置 `span` に label `label` を付けた `invalid_unit_syntax`
fn syntax(span: Span, label: &str) -> EvalError {
  return EvalError::InvalidUnitSyntax {
    span: span.into(),
    label: label.to_string(),
  };
}

#[cfg(test)]
mod tests {
  use crate::{
    document::{HirInlineKind, HirMathKind, HirNodeKind, UnitFactor},
    frontend::{ParseError, evaluator::EvalError, syntax::SyntaxError, test_support},
    source::SourceId,
  };

  /// 段落 1 個の先頭のインラインを評価して返す
  fn first_inline(source: &str) -> HirInlineKind {
    let mut nodes = test_support::parse_for_test(source, SourceId::new(0)).expect("評価に成功するはず").nodes;
    let HirNodeKind::Paragraph(mut inlines) = nodes.swap_remove(0).kind else {
      panic!("Paragraph が期待されます: {source}");
    };
    return inlines.swap_remove(0).kind;
  }

  /// 本文の `\unit{arg}` を評価した因子の列
  fn text_factors(arg: &str) -> Vec<UnitFactor> {
    let HirInlineKind::Unit(factors) = first_inline(&format!("\\unit{{{arg}}}")) else {
      panic!("Unit が期待されます: {arg}");
    };
    return factors;
  }

  /// ソースを評価して評価エラーを取り出す
  fn eval_error(source: &str) -> EvalError {
    match test_support::parse_for_test(source, SourceId::new(0)) {
      Err(ParseError::Eval(error)) => return error,
      other => panic!("評価エラーが期待されます: {source}: {other:?}"),
    }
  }

  /// `\unit{arg}` を本文（`math` なら `$...$` の中）に書いたときの、診断の種類と label の引数の中の（位置, 長さ）
  fn error_at(arg: &str, math: bool) -> (&'static str, usize, usize) {
    let dollar = if math { "$" } else { "" };
    let source = format!("{dollar}\\unit{{{arg}}}{dollar}");
    let (kind, span) = match eval_error(&source) {
      EvalError::UnknownUnit { span, .. } => ("unknown", span),
      EvalError::UnitPrefixNotAllowed { span, .. } => ("prefix", span),
      EvalError::InvalidUnitExponent { span } => ("exponent", span),
      EvalError::InvalidUnitSyntax { span, .. } => ("syntax", span),
      other => panic!("単位の診断が期待されます: {source}: {other:?}"),
    };
    return (kind, span.offset() - (dollar.len() + "\\unit{".len()), span.len());
  }

  /// 字面から因子を作る
  fn factor(symbol: &str, exponent: Option<&str>) -> UnitFactor {
    return UnitFactor {
      symbol: symbol.to_string(),
      exponent: exponent.map(str::to_string),
    };
  }

  #[test]
  fn accepts_units() {
    let cases = [
      ("m", vec![factor("m", None)]),
      ("km.s^{-2}", vec![factor("km", None), factor("s", Some("-2"))]),
      (
        "kg.m^{2}.s^{-2}",
        vec![
          factor("kg", None),
          factor("m", Some("2")),
          factor("s", Some("-2")),
        ],
      ),
      ("\\mu m", vec![factor("\u{03BC}m", None)]),
      ("\u{00B5}m", vec![factor("\u{00B5}m", None)]),
      ("k\\Omega", vec![factor("k\u{03A9}", None)]),
      ("\u{2126}", vec![factor("\u{2126}", None)]),
      ("\\degree C", vec![factor("\u{00B0}C", None)]),
      ("\u{2103}", vec![factor("\u{2103}", None)]),
      ("\u{00B0}.C", vec![factor("\u{00B0}", None), factor("C", None)]),
      ("\\prime", vec![factor("\u{2032}", None)]),
      ("\\doubleprime", vec![factor("\u{2033}", None)]),
      ("%", vec![factor("%", None)]),
      ("m\\degree C", vec![factor("m\u{00B0}C", None)]),
    ];

    for (arg, expected) in cases {
      assert_eq!(text_factors(arg), expected, "{arg}");
    }
  }

  #[test]
  fn rejects_at_the_offending_span() {
    let cases = [
      ("N\u{00B7}m", ("unknown", 0, 4)),
      ("s\u{00B2}", ("unknown", 0, 3)),
      ("bar", ("unknown", 0, 3)),
      ("kmin", ("prefix", 0, 4)),
      ("k%", ("prefix", 0, 2)),
      ("km.mha", ("prefix", 3, 3)),
      ("m^{1}", ("exponent", 2, 3)),
      ("m^{0}", ("exponent", 2, 3)),
      ("m^{+2}", ("exponent", 2, 4)),
      ("m^{02}", ("exponent", 2, 4)),
      ("m^{1/2}", ("exponent", 2, 5)),
      ("m^{}", ("exponent", 2, 2)),
      ("m^{\u{2212}1}", ("exponent", 2, 6)),
      ("m^{-}", ("exponent", 2, 3)),
      ("m s", ("syntax", 1, 1)),
      ("m..s", ("syntax", 2, 1)),
      (".m", ("syntax", 0, 1)),
      ("m.", ("syntax", 1, 1)),
      ("m/s", ("syntax", 1, 1)),
      ("m\\/s", ("syntax", 1, 2)),
      ("kg\\_m", ("syntax", 2, 2)),
      ("kg\\&m", ("syntax", 2, 2)),
      ("^{2}", ("syntax", 0, 1)),
      ("m^{2}^{3}", ("syntax", 5, 1)),
      ("m^{2}s", ("syntax", 5, 1)),
      ("m_{2}", ("syntax", 1, 4)),
      ("{m}", ("syntax", 0, 3)),
      ("\\frac{a}{b}", ("syntax", 0, 11)),
      ("\\num{1}", ("syntax", 0, 7)),
      ("m^ {2}", ("syntax", 2, 1)),
      ("m^{ 2}", ("syntax", 3, 1)),
      ("m.s// c\n", ("syntax", 7, 1)),
    ];

    for (arg, expected) in cases {
      assert_eq!(error_at(arg, false), expected, "本文: {arg:?}");
      assert_eq!(error_at(arg, true), expected, "数式: {arg:?}");
    }
  }

  #[test]
  fn empty_argument_labels_the_whole_argument() {
    let EvalError::InvalidUnitSyntax { span, .. } = eval_error("\\unit{}") else {
      panic!("invalid_unit_syntax が期待されます");
    };

    assert_eq!((span.offset(), span.len()), (5, 2), "`{{}}` 全体");
  }

  #[test]
  fn unknown_command_is_unknown() {
    assert!(matches!(
      eval_error("\\unit{\\metre}"),
      EvalError::UnknownCommand { ref name, .. } if name == "metre"
    ));
  }

  #[test]
  fn parser_errors_come_first() {
    let escaped = test_support::parse_for_test("\\unit{m\\.s}", SourceId::new(0));
    let bare_script = test_support::parse_for_test("\\unit{m^2}", SourceId::new(0));

    assert!(matches!(escaped, Err(ParseError::Syntax(SyntaxError::NeedlessEscape { .. }))), "{escaped:?}");
    assert!(
      matches!(bare_script, Err(ParseError::Syntax(SyntaxError::ScriptRequiresGroup { .. }))),
      "{bare_script:?}"
    );
  }

  #[test]
  fn unit_in_text_is_a_unit_inline() {
    assert!(matches!(first_inline("\\unit{m}"), HirInlineKind::Unit(_)));
  }

  #[test]
  fn unit_in_math_is_a_unit_node() {
    let HirInlineKind::Math(math) = first_inline("$a\\unit{m}$") else {
      panic!("Math が期待されます");
    };

    let kinds: Vec<&HirMathKind> = math.iter().map(|node| return &node.kind).collect();
    assert!(
      matches!(kinds.as_slice(), [HirMathKind::Text(_), HirMathKind::Unit(f)] if f.len() == 1),
      "{kinds:?}"
    );
  }

  #[test]
  fn unit_is_rejected_inside_math_text() {
    assert!(matches!(
      eval_error("$\\text{\\unit{m}}$"),
      EvalError::UnsupportedInMathText { ref what, .. } if what == "\\unit"
    ));
  }

  #[test]
  fn unit_in_index_is_rejected() {
    assert!(matches!(
      eval_error("\\index{\\unit{m}}"),
      EvalError::InvalidCommandArgument { ref name, .. } if name == "index"
    ));
  }

  #[test]
  fn unit_takes_no_options() {
    assert!(matches!(eval_error("\\unit[a=b]{m}"), EvalError::UnknownOptArgKey { .. }));
  }
}
