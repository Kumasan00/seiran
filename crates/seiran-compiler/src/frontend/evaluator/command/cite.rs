//! `\cite{key}` / `\cite{a,b}` コマンド
//!
//! キーの区切りは構造トークンの `,` だけで、`\,` はキーの文字になる。引用キーの存在検証と
//! 表示ラベルの整形は後段に委ねる。

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalContext, EvalError, signature::Signature, string_arg},
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\cite` の署名（キーの区切りは構造トークンの `,`）
pub(super) const SIGNATURE: Signature<1> = Signature::new([ArgMode::CommaList], "引用キー", &[]);

/// `\cite{a,b}` を `HirInlineKind::Cite` に変換する
///
/// 各キーを trim し、空のキー（`\cite{}` や `\cite{a,}` / `\cite{,a}` / `\cite{a,,b}` のような
/// 先頭・末尾・連続カンマ）はエラーとする。
///
/// # Errors
///
/// キーに字以外の要素（コマンド・数式など）がある場合、または空のキーが含まれる場合にエラーを返します。
pub(super) fn cite(view: &CommandView<'_>, arg: &CstNode<'_>, ctx: &EvalContext<'_>) -> Result<HirInline, EvalError> {
  let segments = string_arg::comma_items(view, arg)?;
  let mut keys = Vec::with_capacity(segments.len());
  for segment in segments {
    let key = segment.trim();
    if key.is_empty() {
      return Err(EvalError::InvalidCommandArgument {
        name: "cite".to_string(),
        reason: "空の引用キーが含まれています".to_string(),
        span: view.span().into(),
      });
    }
    keys.push(key.to_string());
  }

  return Ok(ctx.leaf_inline(view.span(), HirInlineKind::Cite { keys }));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_handler, test_support};

  #[test]
  fn cite_produces_single_key() {
    let arena = Bump::new();
    let source = r"\cite{rika}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| return cite(&view, SIGNATURE.bind(&view)?.args[0], ctx)).unwrap();

    let HirInlineKind::Cite { keys } = &result.kind else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["rika".to_string()]);
  }

  #[test]
  fn cite_splits_multiple_keys_and_trims() {
    let arena = Bump::new();
    let source = r"\cite{a, b ,c}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| return cite(&view, SIGNATURE.bind(&view)?.args[0], ctx)).unwrap();

    let HirInlineKind::Cite { keys } = &result.kind else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["a".to_string(), "b".to_string(), "c".to_string()]);
  }

  /// `source` の `\cite` を評価して得たキー列を返す（評価が失敗したらテストを落とす）
  fn cite_keys(source: &str) -> Vec<String> {
    let arena = Bump::new();
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);
    let result = run_handler(|ctx| return cite(&view, SIGNATURE.bind(&view)?.args[0], ctx)).unwrap();
    let HirInlineKind::Cite { keys } = result.kind else {
      panic!("Cite が期待されます");
    };
    return keys;
  }

  /// `source` の `\cite` の評価が `InvalidCommandArgument`（name = cite）で失敗するか
  fn cite_is_invalid_argument(source: &str) -> bool {
    let arena = Bump::new();
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);
    return matches!(
      run_handler(|ctx| return cite(&view, SIGNATURE.bind(&view)?.args[0], ctx)),
      Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite"
    );
  }

  #[test]
  fn cite_splits_on_structural_comma_only() {
    assert_eq!(cite_keys(r"\cite{a\,b, c}"), vec!["a,b".to_string(), "c".to_string()]);
  }

  #[test]
  fn cite_splits_unspaced_keys() {
    assert_eq!(cite_keys(r"\cite{a,b}"), vec!["a".to_string(), "b".to_string()]);
  }

  #[test]
  fn cite_trims_whitespace_and_newline_around_keys() {
    assert_eq!(cite_keys("\\cite{a ,\n b}"), vec!["a".to_string(), "b".to_string()]);
  }

  #[test]
  fn cite_rejects_leading_empty_key() {
    assert!(cite_is_invalid_argument(r"\cite{,a}"));
  }

  #[test]
  fn cite_rejects_consecutive_commas() {
    assert!(cite_is_invalid_argument(r"\cite{a,,b}"));
  }

  #[test]
  fn cite_rejects_empty_argument() {
    assert!(cite_is_invalid_argument(r"\cite{}"));
  }

  #[test]
  fn cite_rejects_empty_key() {
    let arena = Bump::new();
    let source = r"\cite{a,}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_handler(|ctx| return cite(&view, SIGNATURE.bind(&view)?.args[0], ctx)), Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite")
    );
  }
}
