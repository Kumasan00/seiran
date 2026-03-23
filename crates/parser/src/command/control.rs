//! 制御コマンド群
//!
//! スペース挿入などの制御コマンドを提供します。

use crate::{
  ast::{CommandView, extract_text_content},
  document::DocNode,
  evaluator::EvalError,
};

/// `\space{N}` — 垂直スペースを挿入するコマンド
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
///
/// # Errors
///
/// 引数の不足・過剰・数値でない場合にエラーを返します
pub(super) fn space(view: &CommandView) -> Result<Vec<DocNode>, EvalError> {
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "space".to_string(),
      expected: "スペース量（数値）".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 || !view.opt_args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "space".to_string(),
      span: view.span().into(),
    });
  }

  let text = extract_text_content(view.source(), first_arg);
  let trimmed = text.trim();

  if trimmed.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "space".to_string(),
      reason: "数値のみ".to_string(),
      span: view.span().into(),
    });
  }

  let space_value: f32 = match trimmed.parse() {
    Ok(val) => val,
    Err(_) => {
      return Err(EvalError::InvalidCommandArgument {
        name: "space".to_string(),
        reason: "数値".to_string(),
        span: view.span().into(),
      });
    },
  };

  return Ok(vec![DocNode::Space(space_value)]);
}
