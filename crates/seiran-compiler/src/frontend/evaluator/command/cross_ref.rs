//! `\refs{a, b}` / `\refrange{始点}{終点}` コマンド
//!
//! `\refs` のラベルの区切りは構造トークンの `,` だけで、`\,` はラベルの字になる（`\cite` のキーと同じ）。各ラベルを
//! trim し、空のラベル・1 個だけの列・同じラベルの重複を拒否する。並びはソース順のまま整列しない。`\refrange` の
//! 2 つの引数は `\ref` と同じ読み（エスケープを解いた平文を trim）で、両端が同じラベルなら拒否する。ラベルの解決は
//! 後段に委ねる。

use std::collections::HashSet;

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalContext, EvalError, signature::Signature, string_arg},
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\refs` の署名（ラベルの区切りは構造トークンの `,`）
pub(super) const REFS: Signature<1> = Signature::new([ArgMode::CommaList], "ラベル名の列", &[]);
/// `\refrange` の署名（2 つのラベル名はどちらも `\ref` と同じ読み）
pub(super) const REF_RANGE: Signature<2> =
  Signature::new([ArgMode::Inherit, ArgMode::Inherit], "2 個（始点と終点のラベル名）", &[]);

/// `\refs{a, b}` を `HirInlineKind::RefList` に変換する
///
/// # Errors
///
/// ラベルに字以外の要素がある場合、空のラベルが含まれる場合、ラベルが 1 個だけの場合、同じラベルが 2 回以上
/// 並ぶ場合にエラーを返します（検査はこの順）。
pub(super) fn refs(view: &CommandView<'_>, arg: &CstNode<'_>, ctx: &EvalContext<'_>) -> Result<HirInline, EvalError> {
  let segments = string_arg::comma_items(view, arg)?;
  let mut labels = Vec::with_capacity(segments.len());
  for segment in segments {
    let label = segment.trim();
    if label.is_empty() {
      return Err(invalid_argument(view, "空のラベルが含まれています".to_string()));
    }
    labels.push(label.to_string());
  }
  if labels.len() == 1 {
    return Err(invalid_argument(view, r"ラベルが 1 個だけです。1 個の参照は \ref で書いてください".to_string()));
  }
  let mut seen = HashSet::new();
  if let Some(duplicate) = labels.iter().find(|label| return !seen.insert(label.as_str())) {
    return Err(invalid_argument(view, format!("ラベル `{duplicate}` が 2 回以上並んでいます")));
  }

  return Ok(ctx.leaf_inline(view.span(), HirInlineKind::RefList { labels }));
}

/// `\refrange{始点}{終点}` を `HirInlineKind::RefRange` に変換する
///
/// # Errors
///
/// ラベルに字以外の要素がある場合と、始点と終点が同じラベルの場合にエラーを返します。
pub(super) fn ref_range(
  view: &CommandView<'_>,
  [first_arg, last_arg]: [&CstNode<'_>; 2],
  ctx: &EvalContext<'_>,
) -> Result<HirInline, EvalError> {
  let first = string_arg::text(view, first_arg)?.trim().to_string();
  let last = string_arg::text(view, last_arg)?.trim().to_string();
  if first == last {
    return Err(invalid_argument(
      view,
      format!("始点と終点が同じラベル `{first}` です。範囲の両端には別の項目を指定してください"),
    ));
  }

  return Ok(ctx.leaf_inline(view.span(), HirInlineKind::RefRange { first, last }));
}

/// コマンド全体を指す引数エラーを作る
fn invalid_argument(view: &CommandView<'_>, reason: String) -> EvalError {
  return EvalError::InvalidCommandArgument {
    name: view.name().to_string(),
    reason,
    span: view.span().into(),
  };
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_handler, test_support};

  /// `\refs{...}` をハンドラ単体で評価する
  fn eval_refs(source: &str) -> Result<HirInline, EvalError> {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_handler(|ctx| return refs(&view, REFS.bind(&view)?.args[0], ctx));
  }

  /// `\refrange{...}{...}` をハンドラ単体で評価する
  fn eval_ref_range(source: &str) -> Result<HirInline, EvalError> {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_handler(|ctx| return ref_range(&view, REF_RANGE.bind(&view)?.args, ctx));
  }

  /// 評価が `InvalidCommandArgument` で失敗したときの（コマンド名, 理由）
  fn rejection(result: Result<HirInline, EvalError>) -> (String, String) {
    let Err(EvalError::InvalidCommandArgument { name, reason, .. }) = result else {
      panic!("InvalidCommandArgument が期待されます: {result:?}");
    };
    return (name, reason);
  }

  #[test]
  fn refs_reads_comma_separated_labels_in_source_order() {
    let result = eval_refs(r"\refs{ b, a\,x ,c}").unwrap();

    assert_eq!(
      result.kind,
      HirInlineKind::RefList {
        labels: vec!["b".to_string(), "a,x".to_string(), "c".to_string()],
      }
    );
  }

  #[test]
  fn refs_rejects_empty_item() {
    for source in [r"\refs{a,,b}", r"\refs{a, b,}", r"\refs{}"] {
      let (name, reason) = rejection(eval_refs(source));

      assert_eq!(name, "refs", "{source}");
      assert_eq!(reason, "空のラベルが含まれています", "{source}");
    }
  }

  #[test]
  fn refs_rejects_single_label() {
    let (name, reason) = rejection(eval_refs(r"\refs{a}"));

    assert_eq!(name, "refs");
    assert_eq!(reason, r"ラベルが 1 個だけです。1 個の参照は \ref で書いてください");
  }

  #[test]
  fn refs_rejects_duplicate_label() {
    let (name, reason) = rejection(eval_refs(r"\refs{a, b, a}"));

    assert_eq!(name, "refs");
    assert_eq!(reason, "ラベル `a` が 2 回以上並んでいます");
  }

  #[test]
  fn ref_range_reads_two_labels_like_ref() {
    let result = eval_ref_range(r"\refrange{ e\_1 }{e2}").unwrap();

    assert_eq!(
      result.kind,
      HirInlineKind::RefRange {
        first: "e_1".to_string(),
        last: "e2".to_string(),
      }
    );
  }

  #[test]
  fn ref_range_rejects_same_label() {
    let (name, reason) = rejection(eval_ref_range(r"\refrange{a}{ a }"));

    assert_eq!(name, "refrange");
    assert_eq!(reason, "始点と終点が同じラベル `a` です。範囲の両端には別の項目を指定してください");
  }

  #[test]
  fn refs_and_ref_range_reject_options() {
    for source in [r"\refs[k=v]{a, b}", r"\refrange[k=v]{a}{b}"] {
      let result = test_support::evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn refs_inside_math_text_is_rejected() {
    for source in [r"$\text{\refs{a, b}}$", r"$\text{\refrange{a}{b}}$"] {
      let result = test_support::evaluate_document(source);

      assert!(matches!(result, Err(EvalError::UnsupportedInMathText { .. })), "{source}: {result:?}");
    }
  }

  #[test]
  fn refs_inside_link_display_is_rejected() {
    let cases = [
      (r"\href{u}{\refs{a, b}}", r"\refs"),
      (r"\hyperref{x}{\bold{\refrange{a}{b}}}", r"\refrange"),
    ];

    for (source, expected) in cases {
      let result = test_support::evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::LinkInLinkDisplay { ref what, .. }) if what == expected),
        "{source}: {result:?}"
      );
    }
  }
}
