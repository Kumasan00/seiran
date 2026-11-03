//! `\num{数値}` の評価 — 数式として読んだ引数を数値の文法で検証し、数字列のまま [`Numeral`] にする
//!
//! 文法は `[符号] 整数部 [. 小数部] [e 指数]`。引数の構造（空白・コマンド・グループ・上下付き）は parser が読んだまま
//! 受け取り、字のトークン以外の要素はその位置で文法から外れたものとして扱う。

use crate::{
  document::{NumberSign, Numeral},
  frontend::{
    evaluator::{self, EvalError, arity, opt_args},
    syntax::{CstElement, CstNode, token::TokenKind, view::CommandView},
  },
  source::Span,
};

/// `\num` の引数を数値の文法で読む
///
/// # Errors
///
/// 任意引数・引数の個数の誤りと、[`numeral_arg`] のエラーを返す。
pub(in crate::frontend::evaluator) fn numeral(view: &CommandView<'_>) -> Result<Numeral, EvalError> {
  opt_args::no_command_opt_args(view)?;
  let arg = arity::exactly_one_arg(view, "1 個（数値）")?;
  return numeral_arg(view.source(), arg);
}

/// 引数ノード `arg` を数値の文法で読む
///
/// # Errors
///
/// 文法から外れた引数（最初に外れた位置の 1 件。空の引数は引数全体）を返す。
pub(in crate::frontend::evaluator) fn numeral_arg(source: &str, arg: &CstNode<'_>) -> Result<Numeral, EvalError> {
  let items = arg_items(source, arg);
  if items.is_empty() {
    return Err(EvalError::InvalidNumber {
      span: arg.span.into(),
      label: "数値がありません".to_string(),
    });
  }
  // 引数の閉じ括弧の位置（文法が途中で終わったときに指す、引数の中身の末尾）
  let end = arg.span.end - 1;
  return Reader {
    items: &items,
    pos: 0,
    end,
  }
  .numeral();
}

/// 引数の中身を左から読んだ要素
enum Item {
  /// 字のトークンの 1 字（字, ソース上の開始位置）
  Char(char, u32),
  /// 字のトークン以外の要素（空白・コマンド・グループ・上下付きなど）。文法はここで必ず外れる
  Other(Span),
}

/// 引数ノード `arg` の中身を [`Item`] の列にする（引数を区切る括弧とコメントは除く）
fn arg_items(source: &str, arg: &CstNode<'_>) -> Vec<Item> {
  let mut items = Vec::new();
  for child in arg.children {
    match child {
      CstElement::Token(token) => match token.kind {
        TokenKind::Text => {
          for (offset, ch) in token.text(source).char_indices() {
            items.push(Item::Char(ch, evaluator::source_position(token.span.start, offset)));
          }
        },
        TokenKind::LBrace | TokenKind::RBrace | TokenKind::Comment => {},
        TokenKind::VerbatimText
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::Escaped
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
        | TokenKind::Unknown => items.push(Item::Other(token.span)),
      },
      CstElement::Node(node) => items.push(Item::Other(node.span)),
    }
  }
  return items;
}

/// 数値の文法を左から 1 回だけ読む読み手
struct Reader<'a> {
  /// 引数の中身
  items: &'a [Item],
  /// 次に読む要素の位置
  pos: usize,
  /// 引数の中身の末尾（閉じ括弧の位置）
  end: u32,
}

impl Reader<'_> {
  /// 数値全体を読む
  fn numeral(mut self) -> Result<Numeral, EvalError> {
    let sign = match self.peek() {
      Some('-') => Some(NumberSign::Minus),
      Some('+') => Some(NumberSign::Plus),
      _ => None,
    };
    if sign.is_some() {
      self.pos += 1;
    }
    let integer = match self.peek() {
      Some('0') => {
        self.pos += 1;
        "0".to_string()
      },
      Some('1'..='9') => self.digits(),
      _ => return Err(self.fail()),
    };
    // `-0` は負の零の第 2 の綴りにしかならないので、`-` の後の整数部 `0` には小数部が要る
    if sign == Some(NumberSign::Minus) && integer == "0" && self.peek() != Some('.') {
      return Err(self.fail());
    }
    let fraction = if self.peek() == Some('.') {
      self.pos += 1;
      if !self.peek().is_some_and(|ch| return ch.is_ascii_digit()) {
        return Err(self.fail());
      }
      Some(self.digits())
    } else {
      None
    };
    let exponent = if self.peek() == Some('e') {
      self.pos += 1;
      let negative = self.peek() == Some('-');
      if negative {
        self.pos += 1;
      }
      if !matches!(self.peek(), Some('1'..='9')) {
        return Err(self.fail());
      }
      let digits = self.digits();
      Some(if negative {
        format!("-{digits}")
      } else {
        digits
      })
    } else {
      None
    };
    if self.pos < self.items.len() {
      return Err(self.fail());
    }
    return Ok(Numeral {
      sign,
      integer,
      fraction,
      exponent,
    });
  }

  /// 次の要素が字ならその字
  fn peek(&self) -> Option<char> {
    return match self.items.get(self.pos) {
      Some(Item::Char(ch, _)) => Some(*ch),
      Some(Item::Other(_)) | None => None,
    };
  }

  /// ASCII 数字の連なりを読む（0 字でもよい。呼び元が先頭の字を確かめてある）
  fn digits(&mut self) -> String {
    let mut digits = String::new();
    while let Some(ch) = self.peek().filter(char::is_ascii_digit) {
      digits.push(ch);
      self.pos += 1;
    }
    return digits;
  }

  /// 次の要素の位置（中身を読み切っていれば閉じ括弧の位置）で文法から外れた診断
  fn fail(&self) -> EvalError {
    let span = match self.items.get(self.pos) {
      Some(Item::Char(ch, start)) => {
        let len = u32::try_from(ch.len_utf8()).expect("1 字の UTF-8 長は 4 以下");
        Span::new(*start, start + len)
      },
      Some(Item::Other(span)) => *span,
      None => Span::new(self.end, self.end),
    };
    return EvalError::InvalidNumber {
      span: span.into(),
      label: "ここで数値の書き方から外れています".to_string(),
    };
  }
}

#[cfg(test)]
mod tests {
  use crate::{
    document::{HirInlineKind, HirMathKind, HirNodeKind, NumberSign, Numeral},
    frontend::{ParseError, evaluator::EvalError, test_support},
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

  /// 本文の `\num{arg}` を評価した数値
  fn text_numeral(arg: &str) -> Numeral {
    let HirInlineKind::Number(numeral) = first_inline(&format!("\\num{{{arg}}}")) else {
      panic!("Number が期待されます: {arg}");
    };
    return numeral;
  }

  /// ソースを評価して評価エラーを取り出す
  fn eval_error(source: &str) -> EvalError {
    match test_support::parse_for_test(source, SourceId::new(0)) {
      Err(ParseError::Eval(error)) => return error,
      other => panic!("評価エラーが期待されます: {source}: {other:?}"),
    }
  }

  /// `\num{arg}` を本文（`math` なら `$...$` の中）に書いたときの、`invalid_number` の label の引数の中のバイト位置
  fn invalid_at(arg: &str, math: bool) -> usize {
    let dollar = if math { "$" } else { "" };
    let source = format!("{dollar}\\num{{{arg}}}{dollar}");
    let EvalError::InvalidNumber { span, .. } = eval_error(&source) else {
      panic!("invalid_number が期待されます: {source}");
    };
    return span.offset() - (dollar.len() + "\\num{".len());
  }

  /// 部分ごとの字面から数値を作る
  fn numeral(sign: Option<NumberSign>, integer: &str, fraction: Option<&str>, exponent: Option<&str>) -> Numeral {
    return Numeral {
      sign,
      integer: integer.to_string(),
      fraction: fraction.map(str::to_string),
      exponent: exponent.map(str::to_string),
    };
  }

  #[test]
  fn accepts_the_grammar() {
    let minus = Some(NumberSign::Minus);
    let cases = [
      ("0", numeral(None, "0", None, None)),
      ("5", numeral(None, "5", None, None)),
      ("+5", numeral(Some(NumberSign::Plus), "5", None, None)),
      ("+0", numeral(Some(NumberSign::Plus), "0", None, None)),
      ("-5", numeral(minus, "5", None, None)),
      ("1.50", numeral(None, "1", Some("50"), None)),
      ("1.5e3", numeral(None, "1", Some("5"), Some("3"))),
      ("1.5e-3", numeral(None, "1", Some("5"), Some("-3"))),
      ("-0.5", numeral(minus, "0", Some("5"), None)),
      ("12345.6789", numeral(None, "12345", Some("6789"), None)),
      ("2e10", numeral(None, "2", None, Some("10"))),
    ];

    for (arg, expected) in cases {
      assert_eq!(text_numeral(arg), expected, "{arg}");
    }
  }

  #[test]
  fn rejects_at_the_first_offending_byte() {
    let cases = [
      ("007", 1),
      (".5", 0),
      ("5.", 2),
      ("E3", 0),
      ("d3", 0),
      ("1e+3", 2),
      ("1e03", 2),
      ("1e0", 2),
      ("1e-0", 3),
      ("-0", 2),
      ("-0e3", 2),
      ("1 234", 1),
      ("1,5", 1),
      ("1_{5}", 1),
      ("\u{2212}5", 0),
      ("\\minus 5", 0),
      ("1{2}", 1),
      ("1.5e", 4),
      ("-", 1),
    ];

    for (arg, offset) in cases {
      assert_eq!(invalid_at(arg, false), offset, "本文: {arg}");
      assert_eq!(invalid_at(arg, true), offset, "数式: {arg}");
    }
  }

  #[test]
  fn empty_argument_labels_the_whole_argument() {
    let EvalError::InvalidNumber { span, .. } = eval_error("\\num{}") else {
      panic!("invalid_number が期待されます");
    };

    assert_eq!((span.offset(), span.len()), (4, 2), "`{{}}` 全体");
  }

  #[test]
  fn num_in_text_is_a_number_inline() {
    assert!(matches!(first_inline("\\num{-5}"), HirInlineKind::Number(_)));
  }

  #[test]
  fn num_in_math_is_a_number_node() {
    let HirInlineKind::Math(math) = first_inline("$a\\num{-5}$") else {
      panic!("Math が期待されます");
    };

    let kinds: Vec<&HirMathKind> = math.iter().map(|node| return &node.kind).collect();
    assert!(
      matches!(kinds.as_slice(), [HirMathKind::Text(_), HirMathKind::Number(n)] if n.sign == Some(NumberSign::Minus)),
      "{kinds:?}"
    );
  }

  #[test]
  fn num_is_rejected_inside_math_text() {
    assert!(matches!(
      eval_error("$\\text{\\num{1}}$"),
      EvalError::UnsupportedInMathText { ref what, .. } if what == "\\num"
    ));
  }

  #[test]
  fn num_in_index_is_rejected() {
    assert!(matches!(
      eval_error("\\index{\\num{1}}"),
      EvalError::InvalidCommandArgument { ref name, .. } if name == "index"
    ));
  }

  #[test]
  fn num_takes_no_options() {
    assert!(matches!(eval_error("\\num[a=b]{1}"), EvalError::UnknownOptArgKey { .. }));
  }
}
