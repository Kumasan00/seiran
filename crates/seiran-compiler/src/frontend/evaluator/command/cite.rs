//! `\cite{key}` / `\cite{a,b}` コマンド
//!
//! 引用キーの存在検証と表示ラベルの整形は後段に委ねる。

use crate::{
  document::{HirBuilder, HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalError, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::ast::{CommandView, extract_text_content},
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
pub(crate) fn cite_command(view: &CommandView, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "cite".to_string(),
      expected: "引用キー".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "cite".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let raw = extract_text_content(view.source(), first_arg);
  let segments: Vec<&str> = raw.split(',').collect();
  let mut keys = Vec::with_capacity(segments.len());
  for segment in segments {
    let key = segment.trim();
    if key.is_empty() {
      return Err(EvalError::InvalidCommandArgument {
        name: "cite".to_string(),
        reason: "空の引用キーが含まれています".to_string(),
        span: view.span().to_source_span(),
      });
    }
    keys.push(key.to_string());
  }

  return Ok(vec![builder.leaf_inline(view.span(), HirInlineKind::Cite { keys })]);
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
    let result = run_inline_handler(|builder| return cite_command(&view, builder)).unwrap();

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
    let result = run_inline_handler(|builder| return cite_command(&view, builder)).unwrap();

    // Assert
    let HirInlineKind::Cite { keys } = &result[0].kind else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["a".to_string(), "b".to_string(), "c".to_string()]);
  }

  #[test]
  fn cite_rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(
      matches!(run_inline_handler(|builder| return cite_command(&view, builder)), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{a}{b}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(
      matches!(run_inline_handler(|builder| return cite_command(&view, builder)), Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_empty_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{a,}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(
      matches!(run_inline_handler(|builder| return cite_command(&view, builder)), Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite")
    );
  }

  #[test]
  fn cite_rejects_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite[k=v]{rika}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(
      matches!(run_inline_handler(|builder| return cite_command(&view, builder)), Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k")
    );
  }
}
