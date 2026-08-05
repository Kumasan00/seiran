//! スペースや改ページなどの制御コマンド群

use crate::{
  frontend::{
    evaluator::{EvalError, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::ast::{CommandView, extract_text_content},
  },
  model::{HirBuilder, HirNode, HirNodeKind, Length},
};

/// `\space{N}` — 固定幅スペース（pt 単位）を挿入するコマンド
///
/// # Errors
///
/// 引数の不足・過剰・数値でない場合にエラーを返します
pub(super) fn space(view: &CommandView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "space".to_string(),
      expected: "スペース量（数値）".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "space".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let text = extract_text_content(view.source(), first_arg);
  let trimmed = text.trim();

  if trimmed.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "space".to_string(),
      reason: "数値のみ".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let space_value: f32 = match trimmed.parse() {
    Ok(val) => val,
    Err(_) => {
      return Err(EvalError::InvalidCommandArgument {
        name: "space".to_string(),
        reason: "数値".to_string(),
        span: view.span().to_source_span(),
      });
    },
  };

  return Ok(vec![builder.leaf_node(view.span(), HirNodeKind::Space(Length::pt(space_value)))]);
}

/// `\noindent` — 段落先頭行の字下げを抑止するマーカーコマンド
///
/// 段落先頭の位置検証は段落境界を知る `evaluate_children` が行う。
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn noindent(view: &CommandView) -> Result<(), EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }
  return Ok(());
}

/// `\pagebreak` — その位置で強制的に改ページするマーカーコマンド
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn pagebreak(view: &CommandView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }
  return Ok(vec![builder.leaf_node(view.span(), HirNodeKind::PageBreak)]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::{
      evaluator::{lookup_env_parse_mode, run_block_handler},
      syntax::{SyntaxKind, green::GreenElement},
    },
    model::DocNode,
  };

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a crate::frontend::syntax::green::GreenNode<'a> {
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
    // Arrange
    let arena = Bump::new();
    let source = r"\space[draft]{10}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_block_handler(|builder| return space(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn noindent_accepts_no_args() {
    // Arrange
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
    // Arrange
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
    // Arrange
    let arena = Bump::new();
    let source = r"\noindent[draft]";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn pagebreak_produces_page_break_node() {
    // Arrange
    let arena = Bump::new();
    let source = r"\pagebreak";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_block_handler(|builder| return pagebreak(&view, builder));

    // Assert
    assert!(matches!(result.as_deref(), Ok([DocNode::PageBreak])));
  }

  #[test]
  fn pagebreak_rejects_mandatory_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\pagebreak{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_block_handler(|builder| return pagebreak(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "pagebreak"));
  }

  #[test]
  fn pagebreak_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\pagebreak[weight=2]";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_block_handler(|builder| return pagebreak(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "weight"));
  }

  #[test]
  fn pagebreak_splits_surrounding_paragraph() {
    // Arrange
    let arena = Bump::new();
    let source = r"前\pagebreak 後";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children_to_doc_nodes(source, cst).unwrap();

    // Assert
    assert!(matches!(
      result.as_slice(),
      [
        DocNode::Paragraph(_),
        DocNode::PageBreak,
        DocNode::Paragraph(_)
      ]
    ));
  }
}
