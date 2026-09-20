//! `\cite{key}` / `\cite{a,b}` コマンド
//!
//! 引用キーの存在検証と表示ラベルの整形は後段に委ねる。

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalContext, EvalError, arity, opt_args},
    syntax::view::{CommandView, extract_text_content},
  },
};

/// `\cite{a,b}` を `HirInlineKind::Cite` に変換する
///
/// 引数のテキストをカンマで分割し、各キーを trim する。空のキー（`\cite{}` や
/// `\cite{a,}` のような末尾カンマ・連続カンマ）は曖昧さを排除するためエラーとする。
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、任意引数が指定された場合、または空のキーが含まれる場合に
/// エラーを返します。
pub(super) fn cite_command(view: &CommandView<'_>, ctx: &EvalContext<'_>) -> Result<Vec<HirInline>, EvalError> {
  opt_args::no_command_opt_args(view)?;
  let first_arg = arity::exactly_one_arg(view, "引用キー")?;

  let raw = extract_text_content(view.source(), first_arg);
  let segments: Vec<&str> = raw.split(',').collect();
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

  return Ok(vec![ctx.leaf_inline(view.span(), HirInlineKind::Cite { keys })]);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_inline_handler, test_support};

  #[test]
  fn cite_produces_single_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{rika}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|ctx| return cite_command(&view, ctx)).unwrap();

    // Assert
    let HirInlineKind::Cite { keys } = &result[0].kind else {
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
    let result = run_inline_handler(|ctx| return cite_command(&view, ctx)).unwrap();

    // Assert
    let HirInlineKind::Cite { keys } = &result[0].kind else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["a".to_string(), "b".to_string(), "c".to_string()]);
  }

  #[test]
  fn cite_rejects_missing_argument() {
    let arena = Bump::new();
    let source = r"\cite";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_inline_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_extra_arguments() {
    let arena = Bump::new();
    let source = r"\cite{a}{b}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_inline_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_empty_key() {
    let arena = Bump::new();
    let source = r"\cite{a,}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_inline_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_opt_args() {
    let arena = Bump::new();
    let source = r"\cite[k=v]{rika}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(
      matches!(run_inline_handler(|ctx| return cite_command(&view, ctx)), Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k")
    );
  }
}
