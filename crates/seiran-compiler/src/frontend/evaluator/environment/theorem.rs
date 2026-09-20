//! 定理環境 — `theorem` / `lemma` / … / `proof`（10 種）

use crate::{
  document::{HirNode, HirNodeKind, HirProofTarget, TheoremClass},
  frontend::{
    evaluator::{
      self, EvalContext, EvalError, arity,
      opt_args::{self, OptDecl, OptKey, collect_environment_opt_args},
    },
    syntax::view::EnvironmentView,
  },
};

/// 定理環境の `[title=...]`（見出しに添える題）
const TITLE: OptKey<String> = opt_args::string("title");
/// 定理環境の `[label=...]`（`\ref` からの参照用）
const LABEL: OptKey<String> = opt_args::string("label");
/// `proof` 環境の `[of=...]`（証明対象のラベル）
const OF: OptKey<String> = opt_args::string("of");
/// `proof` 環境のスキーマ（`proof` は採番されないのでラベルを取らない）
const PROOF_SCHEMA: &[OptDecl] = &[TITLE.decl(), OF.decl()];
/// `proof` 以外の定理環境のスキーマ
const THEOREM_SCHEMA: &[OptDecl] = &[TITLE.decl(), LABEL.decl()];

/// 定理環境（10 種共通）を評価する
///
/// クラスはレジストリの値が運ぶ（環境名からの再解決はしない）。
///
/// # Errors
///
/// 未知の任意引数キー、余分な必須引数、ラベル重複などが発生した場合にエラーを返します。
pub(super) fn theorem(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
  class: TheoremClass,
) -> Result<Vec<HirNode>, EvalError> {
  let schema = if class == TheoremClass::Proof {
    PROOF_SCHEMA
  } else {
    THEOREM_SCHEMA
  };
  let opts = collect_environment_opt_args(view, schema)?;
  let title = opts.get(TITLE);
  let label = opts.get(LABEL);
  let of_label = opts.get(OF);
  arity::no_environment_args(view)?;

  let id = ctx.alloc(view.span());
  // `[of=...]` は環境ヘッダにあるので、本体より先に ID を確保する
  let of = of_label.map(|label| {
    return HirProofTarget {
      id: ctx.alloc(view.span()),
      label,
    };
  });
  let body = match view.body() {
    Some(body) => evaluator::evaluate_children(view.source(), ctx, body)?,
    None => Vec::new(),
  };

  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::Theorem {
      class,
      title,
      body,
      of,
      label,
    },
  )]);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirInlineKind, TheoremClass},
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  #[test]
  fn theorem_carries_class_and_body_with_no_number() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}本文\end{theorem}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Theorem {
      class,
      title,
      body,
      of,
      label,
      ..
    } = &result[0].kind
    else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(*class, TheoremClass::Theorem);
    assert!(title.is_none());
    assert!(of.is_none());
    assert!(label.is_none());
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0].kind, HirNodeKind::Paragraph(_)));
  }

  #[test]
  fn proof_class_is_structured() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{proof}証明本文\end{proof}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Theorem { class, .. } = &result[0].kind else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(*class, TheoremClass::Proof);
  }

  #[test]
  fn theorem_captures_title() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{theorem}[title=\"ピタゴラスの定理\"]本文\\end{theorem}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Theorem { title, .. } = &result[0].kind else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(title.as_deref(), Some("ピタゴラスの定理"));
  }

  #[test]
  fn theorem_captures_label_without_resolving() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\ref{thm:p}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Theorem { label, .. } = &result[0].kind else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(label.as_deref(), Some("thm:p"));
    let HirNodeKind::Paragraph(inlines) = &result.last().unwrap().kind else {
      panic!("Paragraph が期待されます: {:?}", result.last());
    };
    assert!(
      matches!(inlines.first().map(|inline| return &inline.kind), Some(HirInlineKind::Ref { label }) if label == "thm:p")
    );
  }

  #[test]
  fn proof_of_captures_target_label_without_resolving() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\begin{proof}[of=thm:p]証明\end{proof}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::Theorem { of, .. } = &result[1].kind else {
      panic!("proof の Theorem が期待されます: {:?}", result[1]);
    };
    let of = of.as_ref().expect("of 参照あり");
    assert_eq!(of.label, "thm:p");
  }

  #[test]
  fn theorem_rejects_of_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[of=thm:p]本文\end{theorem}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "of"));
  }

  #[test]
  fn proof_rejects_label_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{proof}[label=pf:1]証明\end{proof}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn theorem_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[foo=1]本文\end{theorem}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn duplicate_theorem_label_is_structured_without_error() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=dup]A\end{theorem}\begin{lemma}[label=dup]B\end{lemma}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let HirNodeKind::Theorem { label: a, .. } = &result[0].kind else {
      panic!("Theorem が期待されます");
    };
    let HirNodeKind::Theorem { label: b, .. } = &result[1].kind else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(a.as_deref(), Some("dup"));
    assert_eq!(b.as_deref(), Some("dup"));
  }
}
