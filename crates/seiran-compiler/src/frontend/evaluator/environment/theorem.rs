//! 定理環境 — `theorem` / `lemma` / … / `proof`（10 種）

use crate::{
  document::{HirNode, HirNodeKind, HirProofTarget, HirTheorem, TheoremClass},
  frontend::{
    evaluator::{
      self, EvalContext, EvalError,
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::Signature,
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

/// 定理環境の署名（`proof` だけがラベルの代わりに証明対象を受け付ける）
pub(super) fn signature(class: TheoremClass) -> Signature<0> {
  return if class == TheoremClass::Proof {
    Signature::keys_only(PROOF_SCHEMA)
  } else {
    Signature::keys_only(THEOREM_SCHEMA)
  };
}

/// 定理環境（10 種共通）を評価する
///
/// # Errors
///
/// 本体の評価エラーを返します。
pub(super) fn theorem(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
  opts: &OptArgs,
  class: TheoremClass,
) -> Result<HirNode, EvalError> {
  let title = opts.get(TITLE);
  let label = opts.get(LABEL);
  let of_label = opts.get(OF);

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

  return Ok(HirNode::new(
    id,
    HirNodeKind::Theorem(HirTheorem {
      class,
      title,
      body,
      of,
      label,
    }),
  ));
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
    let arena = Bump::new();
    let source = r"\begin{theorem}本文\end{theorem}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 1);
    let HirNodeKind::Theorem(theorem) = &result[0].kind else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(theorem.class, TheoremClass::Theorem);
    assert!(theorem.title.is_none());
    assert!(theorem.of.is_none());
    assert!(theorem.label.is_none());
    assert_eq!(theorem.body.len(), 1);
    assert!(matches!(&theorem.body[0].kind, HirNodeKind::Paragraph(_)));
  }

  #[test]
  fn proof_class_is_structured() {
    let arena = Bump::new();
    let source = r"\begin{proof}証明本文\end{proof}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Theorem(theorem) = &result[0].kind else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(theorem.class, TheoremClass::Proof);
  }

  #[test]
  fn theorem_captures_title() {
    let arena = Bump::new();
    let source = "\\begin{theorem}[title=ピタゴラスの定理]本文\\end{theorem}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Theorem(theorem) = &result[0].kind else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(theorem.title.as_deref(), Some("ピタゴラスの定理"));
  }

  #[test]
  fn theorem_title_accepts_escaped_comma() {
    let arena = Bump::new();
    let source = r"\begin{theorem}[title=a\, b]本文\end{theorem}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Theorem(theorem) = &result[0].kind else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(theorem.title.as_deref(), Some("a, b"));
  }

  #[test]
  fn theorem_captures_label_without_resolving() {
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\ref{thm:p}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Theorem(theorem) = &result[0].kind else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(theorem.label.as_deref(), Some("thm:p"));
    let HirNodeKind::Paragraph(inlines) = &result.last().unwrap().kind else {
      panic!("Paragraph が期待されます: {:?}", result.last());
    };
    assert!(
      matches!(inlines.first().map(|inline| return &inline.kind), Some(HirInlineKind::Ref { label }) if label == "thm:p")
    );
  }

  #[test]
  fn proof_of_captures_target_label_without_resolving() {
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\begin{proof}[of=thm:p]証明\end{proof}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Theorem(theorem) = &result[1].kind else {
      panic!("proof の Theorem が期待されます: {:?}", result[1]);
    };
    let of = theorem.of.as_ref().expect("of 参照あり");
    assert_eq!(of.label, "thm:p");
  }

  #[test]
  fn theorem_rejects_of_key() {
    let arena = Bump::new();
    let source = r"\begin{theorem}[of=thm:p]本文\end{theorem}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "of"));
  }

  #[test]
  fn proof_rejects_label_key() {
    let arena = Bump::new();
    let source = r"\begin{proof}[label=pf:1]証明\end{proof}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn duplicate_theorem_label_is_structured_without_error() {
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=dup]A\end{theorem}\begin{lemma}[label=dup]B\end{lemma}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 2);
    let HirNodeKind::Theorem(first) = &result[0].kind else {
      panic!("Theorem が期待されます");
    };
    let HirNodeKind::Theorem(second) = &result[1].kind else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(first.label.as_deref(), Some("dup"));
    assert_eq!(second.label.as_deref(), Some("dup"));
  }
}
