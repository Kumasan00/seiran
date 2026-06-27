//! 制御コマンド群
//!
//! スペース挿入などの制御コマンドを提供します。

use document::DocNode;
use syntax::ast::{CommandView, extract_text_content};
use types::Length;

use crate::evaluator::{EvalError, opt_args::collect_command_opt_args};

/// `\space{N}` — 固定幅スペース（pt 単位）を挿入するコマンド
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

  return Ok(vec![DocNode::Space(Length::pt(space_value))]);
}

/// `\noindent` — 段落先頭行の字下げを抑止するマーカーコマンド
///
/// 引数も任意引数も取らない（`\notag` と同じ引数なしマーカー）。「段落の先頭にのみ置ける」
/// という位置検証は段落境界を知る `evaluate_children` が担うため、ここでは引数の不在だけを
/// 検証し、結果は呼び出し側（`CommandKind::execute`）が `CommandResult::NoIndent` に詰める。
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn noindent(view: &CommandView) -> Result<(), EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
  return Ok(());
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::{SyntaxKind, green::GreenElement};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

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

  #[test]
  fn noindent_accepts_no_args() {
    // Arrange — 引数なしの `\noindent` は受理される
    let arena = Bump::new();
    let source = r"\noindent";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(result.is_ok());
  }

  #[test]
  fn noindent_rejects_mandatory_argument() {
    // Arrange — `\noindent{x}` は引数過剰でエラー
    let arena = Bump::new();
    let source = r"\noindent{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "noindent"));
  }

  #[test]
  fn noindent_rejects_unknown_opt_arg_key() {
    // Arrange — `\noindent` は任意引数を受け付けない
    let arena = Bump::new();
    let source = r"\noindent[draft]";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }
}
