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
//! 採番・ラベル登録（cleveref）は `CounterRegistry::increment_theorem_with_label` が担う。
//! 見出し書式・本文フォント・QED マーク配置は `lowering` 層がクラスの `read_style::TheoremStyle` を
//! 参照して決める（本ハンドラは番号・サブタイトル・本体・`of` 参照を構造化するだけ）。

use document::{DocNode, ProofTarget, TheoremClass};
use syntax::ast::EnvironmentView;

use crate::evaluator::{
  EvalError, Evaluator,
  opt_args::{OptType, OptValue, collect_environment_opt_args},
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
pub(super) fn theorem(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
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
      _ => {},
    }
  }

  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }

  // 採番（`proof` 等の unnumbered クラスは None）とラベル登録（cleveref）を同時に行う。
  let number = evaluator.registry.increment_theorem_with_label(class, label.as_deref(), view.span().into())?;

  // 本体は通常の本文と同様に再帰評価する（段落・数式・リスト等を含められる）。
  let body = match view.body() {
    Some(body) => evaluator.evaluate_children(view.source(), body)?,
    None => Vec::new(),
  };

  // `of` は pass2（resolve_refs）で対象定理の cleveref 文字列に解決する。
  let of = of_label.map(|label| ProofTarget {
    label,
    number: None,
    span: view.span().into(),
  });

  return Ok(vec![DocNode::Theorem {
    class,
    number,
    title,
    body,
    of,
    label,
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::{InlineNode, TheoremClass};

  use super::*;
  use crate::evaluator::{counter::resolve_refs, lookup_env_parse_mode};

  /// テスト用 `parse` ラッパ
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn theorem_is_numbered_and_carries_class_and_body() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}本文\end{theorem}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let DocNode::Theorem {
      class,
      number,
      title,
      body,
      of,
      label,
    } = &result[0]
    else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(*class, TheoremClass::Theorem);
    assert_eq!(number.as_deref(), Some("1"));
    assert!(title.is_none());
    assert!(of.is_none());
    assert!(label.is_none());
    // 本体は段落としてローワリングされる
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0], DocNode::Paragraph(_)));
  }

  #[test]
  fn theorem_and_lemma_share_the_theorem_counter() {
    // Arrange — 既定では lemma は counter "theorem" を共有するため連番になる
    let arena = Bump::new();
    let source = r"\begin{theorem}A\end{theorem}\begin{lemma}B\end{lemma}\begin{theorem}C\end{theorem}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let numbers: Vec<Option<&str>> = result
      .iter()
      .map(|n| match n {
        DocNode::Theorem { number, .. } => number.as_deref(),
        _ => panic!("Theorem が期待されます: {n:?}"),
      })
      .collect();
    assert_eq!(numbers, vec![Some("1"), Some("2"), Some("3")]);
  }

  #[test]
  fn definition_uses_its_own_counter() {
    // Arrange — definition は既定で counter "definition" を持ち、theorem とは独立採番
    let arena = Bump::new();
    let source = r"\begin{theorem}A\end{theorem}\begin{definition}B\end{definition}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — theorem=1、definition=1（別カウンタ）
    let DocNode::Theorem { number: thm, .. } = &result[0] else {
      panic!("Theorem が期待されます");
    };
    let DocNode::Theorem { number: def, .. } = &result[1] else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(thm.as_deref(), Some("1"));
    assert_eq!(def.as_deref(), Some("1"));
  }

  #[test]
  fn proof_is_unnumbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{proof}証明本文\end{proof}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Theorem { class, number, .. } = &result[0] else {
      panic!("Theorem が期待されます: {:?}", result[0]);
    };
    assert_eq!(*class, TheoremClass::Proof);
    assert!(number.is_none());
  }

  #[test]
  fn theorem_captures_title() {
    // Arrange — title はクォートで囲まれた文字列値
    let arena = Bump::new();
    let source = "\\begin{theorem}[title=\"ピタゴラスの定理\"]本文\\end{theorem}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::Theorem { title, .. } = &result[0] else {
      panic!("Theorem が期待されます");
    };
    assert_eq!(title.as_deref(), Some("ピタゴラスの定理"));
  }

  #[test]
  fn theorem_label_resolves_to_cleveref_string() {
    // Arrange — label を付けた theorem を `\ref` すると「Theorem 1」に解決される
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\ref{thm:p}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let mut result = evaluator.evaluate_children(source, cst).unwrap();
    resolve_refs(&mut result, &evaluator.registry).unwrap();

    // Assert — 末尾の段落に Ref があり、cleveref 書式で解決される
    let DocNode::Paragraph(inlines) = result.last().unwrap() else {
      panic!("Paragraph が期待されます: {:?}", result.last());
    };
    let resolved = inlines.iter().find_map(|n| match n {
      InlineNode::Ref { number, .. } => number.as_deref(),
      _ => None,
    });
    assert_eq!(resolved, Some("Theorem 1"));
  }

  #[test]
  fn proof_of_resolves_target_theorem() {
    // Arrange — proof[of=thm:p] が対象定理の cleveref 文字列に解決される
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=thm:p]本文\end{theorem}\begin{proof}[of=thm:p]証明\end{proof}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let mut result = evaluator.evaluate_children(source, cst).unwrap();
    resolve_refs(&mut result, &evaluator.registry).unwrap();

    // Assert
    let DocNode::Theorem { of, .. } = &result[1] else {
      panic!("proof の Theorem が期待されます: {:?}", result[1]);
    };
    let of = of.as_ref().expect("of 参照あり");
    assert_eq!(of.label, "thm:p");
    assert_eq!(of.number.as_deref(), Some("Theorem 1"));
  }

  #[test]
  fn theorem_rejects_of_key() {
    // Arrange — `of` は proof 専用。theorem では不明キー
    let arena = Bump::new();
    let source = r"\begin{theorem}[of=thm:p]本文\end{theorem}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "of"));
  }

  #[test]
  fn proof_rejects_label_key() {
    // Arrange — proof は採番なしなので label を受け付けない（不明キー）
    let arena = Bump::new();
    let source = r"\begin{proof}[label=pf:1]証明\end{proof}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn theorem_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{theorem}[foo=1]本文\end{theorem}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn duplicate_theorem_label_errors() {
    // Arrange — 同名ラベルの二重定義は DuplicateLabel
    let arena = Bump::new();
    let source = r"\begin{theorem}[label=dup]A\end{theorem}\begin{lemma}[label=dup]B\end{lemma}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::DuplicateLabel { ref label, .. }) if label == "dup"));
  }
}
