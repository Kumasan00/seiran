//! `\cite{key}` / `\cite{a,b}` コマンド
//!
//! キーの区切りは構造トークンの `,` だけで、`\,` はキーの文字になる（#731）。引用キーの存在検証と
//! 表示ラベルの整形は後段に委ねる。

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalContext, EvalError, arity, opt_args},
    syntax::view::{CommandView, split_text_on_commas},
  },
};

/// `\cite{a,b}` を `HirInlineKind::Cite` に変換する
///
/// 引数を構造トークンの `,` で割り（`\,` はキーの文字。#687 と同じ規則）、各キーを trim する。空のキー
/// （`\cite{}` や `\cite{a,}` / `\cite{,a}` / `\cite{a,,b}` のような先頭・末尾・連続カンマ）は曖昧さを
/// 排除するためエラーとする。
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、任意引数が指定された場合、または空のキーが含まれる場合に
/// エラーを返します。
pub(super) fn cite_command(view: &CommandView<'_>, ctx: &EvalContext<'_>) -> Result<HirInline, EvalError> {
  opt_args::no_command_opt_args(view)?;
  let first_arg = arity::exactly_one_arg(view, "引用キー")?;

  let segments = split_text_on_commas(view.source(), first_arg);
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
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{rika}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return cite_command(&view, ctx)).unwrap();

    // Assert
    let HirInlineKind::Cite { keys } = &result.kind else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["rika".to_string()]);
  }

  #[test]
  fn cite_splits_multiple_keys_and_trims() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{a, b ,c}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return cite_command(&view, ctx)).unwrap();

    // Assert
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
    let result = run_handler(|ctx| return cite_command(&view, ctx)).unwrap();
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
      run_handler(|ctx| return cite_command(&view, ctx)),
      Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite"
    );
  }

  #[test]
  fn cite_keeps_escaped_comma_in_key() {
    // `\,` はキーの区切りではなくキーの文字（#731。#687 と同じ規則）
    assert_eq!(cite_keys(r"\cite{a\,b}"), vec!["a,b".to_string()]);
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
  fn cite_does_not_split_on_comma_in_nested_node() {
    // 直下の構造 `,` だけが区切り。入れ子ノードの中身はキーの文字として平坦化する
    assert_eq!(cite_keys(r"\cite{\bold{a,b}}"), vec!["a,b".to_string()]);
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
  fn cite_rejects_missing_argument() {
    let arena = Bump::new();
    let source = r"\cite";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_extra_arguments() {
    let arena = Bump::new();
    let source = r"\cite{a}{b}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_empty_key() {
    let arena = Bump::new();
    let source = r"\cite{a,}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_opt_args() {
    let arena = Bump::new();
    let source = r"\cite[k=v]{rika}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k")
    );
  }
}
