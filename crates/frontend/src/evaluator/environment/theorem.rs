//! 定理環境 — `theorem` / `lemma` / … / `proof`（10 種）

use model::{DocNode, ProofTarget, TheoremClass};

use crate::{
  evaluator::{
    EvalError,
    opt_args::{OptType, OptValue, collect_environment_opt_args},
  },
  span_ext::ToSourceSpan,
  syntax::ast::EnvironmentView,
};

/// 定理環境（10 種共通）を評価する
///
/// # Errors
///
/// 未知の任意引数キー、余分な必須引数、ラベル重複などが発生した場合にエラーを返します。
pub(super) fn theorem(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
  let class =
    TheoremClass::from_name(view.name()).expect("ENVIRONMENTS は 10 種の定理クラスのみを本ハンドラに登録する");

  let schema: &[(&str, OptType)] = if class == TheoremClass::Proof {
    &[("title", OptType::String), ("of", OptType::String)]
  } else {
    &[("title", OptType::String), ("label", OptType::String)]
  };
  let opt_args = collect_environment_opt_args(view, schema)?;

  let mut title: Option<String> = None;
  let mut label: Option<String> = None;
  let mut of_label: Option<String> = None;
  for (key, value) in opt_args {
    let OptValue::String(s) = value else {
      continue;
    };
    match key.as_str() {
      "title" => title = Some(s),
      "label" => label = Some(s),
      "of" => of_label = Some(s),
      _ => unreachable!("collect_environment_opt_args が未知キーを弾くのでここには来ない"),
    }
  }

  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }

  let body = match view.body() {
    Some(body) => crate::evaluator::evaluate_children(view.source(), body)?,
    None => Vec::new(),
  };

  let of = of_label.map(|label| {
    return ProofTarget {
      label,
      span: view.span(),
    };
  });

  return Ok(vec![DocNode::Theorem {
    class,
    title,
    body,
    of,
    label,
    span: view.span(),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use model::TheoremClass;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn theorem_carries_class_and_body_with_no_number() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}本文\end{theorem}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let DocNode::Theorem {
      class,
      title,
      body,
      of,
      label,
      ..
    } = &result[0]
    else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(*class, TheoremClass::Theorem);
    assert!(title.is_none());
    assert!(of.is_none());
    assert!(label.is_none());
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0], DocNode::Paragraph(_)));
  }

  #[test]
  fn proof_class_is_structured() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{proof}証明本文\end{proof}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Theorem { class, .. } = &result[0] else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(*class, TheoremClass::Proof);
  }

  #[test]
  fn theorem_captures_title() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{theorem}[title=\"ピタゴラスの定理\"]本文\\end{theorem}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Theorem { title, .. } = &result[0] else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(title.as_deref(), Some("ピタゴラスの定理"));
  }

  #[test]
  fn theorem_captures_label_without_resolving() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\ref{thm:p}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Theorem { label, .. } = &result[0] else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(label.as_deref(), Some("thm:p"));
    let DocNode::Paragraph(inlines) = result.last().unwrap() else {
      panic!("Paragraph が期待されます: {:?}", result.last());
    };
    assert!(matches!(inlines.first(), Some(model::InlineNode::Ref { label, .. }) if label == "thm:p"));
  }

  #[test]
  fn proof_of_captures_target_label_without_resolving() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\begin{proof}[of=thm:p]証明\end{proof}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Theorem { of, .. } = &result[1] else {
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
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "of"));
  }

  #[test]
  fn proof_rejects_label_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{proof}[label=pf:1]証明\end{proof}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn theorem_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[foo=1]本文\end{theorem}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn duplicate_theorem_label_is_structured_without_error() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=dup]A\end{theorem}\begin{lemma}[label=dup]B\end{lemma}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let DocNode::Theorem { label: a, .. } = &result[0] else {
      panic!("Theorem が期待されます");
    };
    let DocNode::Theorem { label: b, .. } = &result[1] else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(a.as_deref(), Some("dup"));
    assert_eq!(b.as_deref(), Some("dup"));
  }
}
