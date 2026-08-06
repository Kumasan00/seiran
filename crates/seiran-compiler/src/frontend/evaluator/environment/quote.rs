//! 引用環境 — `quote` / `quotation`

use crate::{
  document::{HirBuilder, HirNode, HirNodeKind, QuoteKind},
  frontend::{
    evaluator::{EvalError, opt_args::collect_environment_opt_args},
    span_ext::ToSourceSpan,
    syntax::ast::EnvironmentView,
  },
};

/// 引用環境（`quote` / `quotation`）を評価する
///
/// # Errors
///
/// 任意引数が指定された場合、または余分な必須引数がある場合にエラーを返します。
pub(super) fn quote(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let kind = QuoteKind::from_name(view.name()).expect("ENVIRONMENTS は quote / quotation のみを本ハンドラに登録する");

  let _opt_args = collect_environment_opt_args(view, &[])?;
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }

  let id = builder.alloc(view.span());
  let body = match view.body() {
    Some(body) => crate::frontend::evaluator::evaluate_children(view.source(), builder, body)?,
    None => Vec::new(),
  };

  return Ok(vec![HirNode::new(id, HirNodeKind::Quote { kind, body })]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{evaluate_children_to_hir, lookup_env_parse_mode};

  /// テスト用 `parse` ラッパ
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn quote_carries_kind_and_body() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quote}引用本文\end{quote}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Quote { kind, body } = &result[0].kind else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, QuoteKind::Quote);
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0].kind, HirNodeKind::Paragraph(_)));
  }

  #[test]
  fn quotation_resolves_to_quotation_kind() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quotation}引用本文\end{quotation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Quote { kind, .. } = &result[0].kind else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, QuoteKind::Quotation);
  }

  #[test]
  fn quote_body_can_contain_multiple_paragraphs() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{quote}第一段落\n\n第二段落\\end{quote}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Quote { body, .. } = &result[0].kind else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    let paragraphs = body.iter().filter(|n| matches!(n.kind, HirNodeKind::Paragraph(_))).count();
    assert_eq!(paragraphs, 2, "本体は 2 段落: {body:?}");
  }

  #[test]
  fn quote_rejects_extra_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quote}{余分}本文\end{quote}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraEnvironmentArgument { ref name, .. }) if name == "quote"));
  }

  #[test]
  fn quote_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quote}[foo=1]本文\end{quote}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }
}
