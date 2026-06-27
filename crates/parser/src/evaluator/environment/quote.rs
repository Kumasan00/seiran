//! 引用環境 — `quote` / `quotation`
//!
//! `\begin{quote}...\end{quote}` および `\begin{quotation}...\end{quotation}` を
//! [`DocNode::Quote`] に変換します。環境名から [`QuoteKind`] を解決し、両環境とも本ハンドラに
//! 登録されています。本体（`...`）は通常の本文と同様に再帰評価されます（段落・リスト・数式などを
//! 含められる）。
//!
//! 任意引数・必須引数は受け付けません（左右インデント量・上下マージン・段落先頭字下げ量は
//! `read_style::QuoteStyle` 側で決まり、ソースには現れない）。

use document::{DocNode, QuoteKind};
use syntax::ast::EnvironmentView;

use crate::evaluator::{EvalError, Evaluator, opt_args::collect_environment_opt_args};

/// 引用環境（`quote` / `quotation`）を評価する
///
/// 環境名から [`QuoteKind`] を解決し、本体を再帰評価して [`DocNode::Quote`] を 1 つ返す。
///
/// # Errors
///
/// 任意引数が指定された場合、または余分な必須引数がある場合にエラーを返します。
pub(super) fn quote(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  let kind = QuoteKind::from_name(view.name()).expect("ENVIRONMENTS は quote / quotation のみを本ハンドラに登録する");

  let _opt_args = collect_environment_opt_args(view, &[])?;
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }

  // 本体は通常の本文と同様に再帰評価する（段落・リスト・数式等を含められる）。
  let body = match view.body() {
    Some(body) => evaluator.evaluate_children(view.source(), body)?,
    None => Vec::new(),
  };

  return Ok(vec![DocNode::Quote { kind, body }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::QuoteKind;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn quote_carries_kind_and_body() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quote}引用本文\end{quote}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — kind=Quote、本体は段落 1 つ
    assert_eq!(result.len(), 1);
    let DocNode::Quote { kind, body } = &result[0] else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, QuoteKind::Quote);
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0], DocNode::Paragraph(_)));
  }

  #[test]
  fn quotation_resolves_to_quotation_kind() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quotation}引用本文\end{quotation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Quote { kind, .. } = &result[0] else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, QuoteKind::Quotation);
  }

  #[test]
  fn quote_body_can_contain_multiple_paragraphs() {
    // Arrange — 段落区切り（空行）で 2 段落
    let arena = Bump::new();
    let source = "\\begin{quote}第一段落\n\n第二段落\\end{quote}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 本体に段落が 2 つ
    let DocNode::Quote { body, .. } = &result[0] else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    let paragraphs = body.iter().filter(|n| matches!(n, DocNode::Paragraph(_))).count();
    assert_eq!(paragraphs, 2, "本体は 2 段落: {body:?}");
  }

  #[test]
  fn quote_rejects_extra_argument() {
    // Arrange — quote は必須引数を取らない
    let arena = Bump::new();
    let source = r"\begin{quote}{余分}本文\end{quote}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraEnvironmentArgument { ref name, .. }) if name == "quote"));
  }

  #[test]
  fn quote_rejects_unknown_opt_key() {
    // Arrange — quote は任意引数を取らない
    let arena = Bump::new();
    let source = r"\begin{quote}[foo=1]本文\end{quote}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }
}
