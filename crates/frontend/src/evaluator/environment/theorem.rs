//! 定理環境 — `theorem` / `lemma` / … / `proof`（10 種）
//!
//! `\begin{<class>}[...]...\end{<class>}` を [`DocNode::Theorem`] に変換します。`<class>` は
//! [`TheoremClass`] の 10 種で、すべて本ハンドラに登録され、環境名からクラスを解決します。
//! 本体（`...`）は通常の本文と同様に再帰評価されます（数式・リスト・段落などを含められる）。
//!
//! ## 任意引数
//!
//! - `[title="..."]` — サブタイトル（見出しの `{title}` に反映）。全クラス共通（任意）
//! - `[label=thm:foo]` — `\ref` 解決用ラベル。採番ありクラスのみ（`proof` では受け付けない）
//! - `[of=thm:foo]` — 証明対象の定理を指す参照。`proof` 専用（採番ありクラスでは受け付けない）
//!
//! 採番・ラベル登録（cleveref）・見出し書式・本文フォント・QED マーク配置は、いずれも `lowering` 層が
//! クラスの `config::TheoremStyle` を参照して決める（本ハンドラはクラス・サブタイトル・本体・
//! `of` 参照を構造化するだけで、番号・書式情報は一切持たない）。

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
/// 環境名から [`TheoremClass`] を解決し、`proof` は `[title]` / `[of]`、それ以外は
/// `[title]` / `[label]` の任意引数を受け付ける。`CounterRegistry` で採番（`proof` は採番なし）
/// したうえで本体を再帰評価し、[`DocNode::Theorem`] を 1 つ返す。
///
/// # Errors
///
/// 未知の任意引数キー、余分な必須引数、ラベル重複などが発生した場合にエラーを返します。
pub(super) fn theorem(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
  let class =
    TheoremClass::from_name(view.name()).expect("ENVIRONMENTS は 10 種の定理クラスのみを本ハンドラに登録する");

  // proof は証明対象を指す `of`、採番ありクラスは参照ラベル `label` を受ける。`title` は共通。
  let schema: &[(&str, OptType)] = if class == TheoremClass::Proof {
    &[("title", OptType::String), ("of", OptType::String)]
  } else {
    &[("title", OptType::String), ("label", OptType::String)]
  };
  let opt_args = collect_environment_opt_args(view, schema)?;

  // スキーマで検証済みのキーのみが入る。順序非依存で title / label / of を取り出す。
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

  // 本体は通常の本文と同様に再帰評価する（段落・数式・リスト等を含められる）。
  let body = match view.body() {
    Some(body) => crate::evaluator::evaluate_children(view.source(), body)?,
    None => Vec::new(),
  };

  // `of` の解決（対象定理の cleveref 文字列）は lowering 層の pass2 が担う。
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
    // Arrange — 採番（number）は持たず、lowering 層が発番する
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
    // 本体は段落としてローワリングされる
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0], DocNode::Paragraph(_)));
  }

  #[test]
  fn proof_class_is_structured() {
    // Arrange — 採番の有無（unnumbered）は lowering 層が TheoremStyle から判定する
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
    // Arrange — title はクォートで囲まれた文字列値
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
    // Arrange — label は構造化されるだけで、\ref の解決（cleveref 文字列化）は lowering 層が行う
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
    // Arrange — proof[of=thm:p] は対象ラベルを構造化するだけ（解決は lowering 層）
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
    // Arrange — `of` は proof 専用。theorem では不明キー
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
    // Arrange — proof は採番なしなので label を受け付けない（不明キー）
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
    // Arrange — 同名ラベルの重複検出は lowering 層（CounterRegistry）の責務。
    // parser は両方の label をそのまま構造化するだけでエラーにしない。
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
