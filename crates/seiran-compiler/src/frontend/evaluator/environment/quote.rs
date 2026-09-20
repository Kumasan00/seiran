//! 引用環境 — `quote` / `quotation`

use crate::{
  document::{HirNode, HirNodeKind, HirQuote, QuoteKind},
  frontend::{
    evaluator::{self, EvalContext, EvalError, arity, opt_args},
    syntax::view::EnvironmentView,
  },
};

/// 引用環境（`quote` / `quotation`）を評価する
///
/// 種別はレジストリの値が運ぶ（環境名からの再解決はしない）。
///
/// # Errors
///
/// 任意引数が指定された場合、または余分な必須引数がある場合にエラーを返します。
pub(super) fn quote(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>, kind: QuoteKind) -> Result<HirNode, EvalError> {
  opt_args::no_environment_opt_args(view)?;
  arity::no_environment_args(view)?;

  let id = ctx.alloc(view.span());
  let body = match view.body() {
    Some(body) => evaluator::evaluate_children(view.source(), ctx, body)?,
    None => Vec::new(),
  };

  return Ok(HirNode::new(id, HirNodeKind::Quote(HirQuote { kind, body })));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{evaluate_children_to_hir, test_support};

  #[test]
  fn quote_carries_kind_and_body() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quote}引用本文\end{quote}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Quote(quote) = &result[0].kind else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    assert_eq!(quote.kind, QuoteKind::Quote);
    assert_eq!(quote.body.len(), 1);
    assert!(matches!(&quote.body[0].kind, HirNodeKind::Paragraph(_)));
  }

  #[test]
  fn quotation_resolves_to_quotation_kind() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quotation}引用本文\end{quotation}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Quote(quote) = &result[0].kind else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    assert_eq!(quote.kind, QuoteKind::Quotation);
  }

  #[test]
  fn quote_body_can_contain_multiple_paragraphs() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{quote}第一段落\n\n第二段落\\end{quote}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Quote(quote) = &result[0].kind else {
      panic!("Quote が期待されます: {:?}", result[0]);
    };
    let paragraphs = quote.body.iter().filter(|n| matches!(n.kind, HirNodeKind::Paragraph(_))).count();
    assert_eq!(paragraphs, 2, "本体は 2 段落: {:?}", quote.body);
  }

  #[test]
  fn quote_rejects_extra_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{quote}{余分}本文\end{quote}";
    let cst = test_support::parse(source, &arena).unwrap();

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
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }
}
