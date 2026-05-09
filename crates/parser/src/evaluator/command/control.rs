//! 制御コマンド群
//!
//! スペース挿入などの制御コマンドを提供します。

use syntax::ast::{CommandView, extract_text_content};

use crate::{
  document::DocNode,
  evaluator::{EvalError, opt_args::collect_command_opt_args},
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
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "space".to_string(),
      expected: "スペース量（数値）".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::{SyntaxKind, green::GreenElement, parse};

  use super::*;

  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a syntax::green::GreenNode<'a> {
    let cst = parse(source, arena).unwrap();
    for child in cst.children {
      if let GreenElement::Node(n) = child
        && n.kind == SyntaxKind::CommandCall
      {
        return n;
      }
    }
    panic!("CommandCall ノードが見つかりません");
  }

  #[test]
  fn space_rejects_unknown_opt_arg_key() {
    // Arrange — `\space` は任意引数を受け付けないので未知キーで UnknownOptArgKey
    let arena = Bump::new();
    let source = r"\space[draft]{10}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = space(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }
}
