//! スペースや改ページなどの制御コマンド群

use crate::{
  document::{HirNode, HirNodeKind},
  frontend::{
    evaluator::{EvalContext, EvalError, arity, opt_args},
    syntax::view::{CommandView, extract_text_content},
  },
  length::Length,
};

/// `\space{<長さ>}` — 固定幅スペースを挿入するコマンド
///
/// 長さの書式は config / style と同じ（[`Length`] の `FromStr`）。値域は制限しない（負値は詰め）。
///
/// # Errors
///
/// 引数の不足・過剰・長さとして読めない場合にエラーを返します
pub(super) fn space(view: &CommandView<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
  opt_args::no_command_opt_args(view)?;
  let first_arg = arity::exactly_one_arg(view, "スペース量（長さ）")?;
  let text = extract_text_content(view.source(), first_arg);

  let Ok(length) = text.parse::<Length>() else {
    return Err(EvalError::InvalidCommandArgument {
      name: "space".to_string(),
      reason: "長さ（`<数値>pt` / `<数値>mm` / `<数値>cm`）".to_string(),
      span: view.span().into(),
    });
  };

  return Ok(ctx.leaf_node(view.span(), HirNodeKind::Space(length)));
}

/// `\noindent` — 段落先頭行の字下げを抑止するマーカーコマンド
///
/// 段落先頭の位置検証は段落境界を知る `evaluate_children` が行う。
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn noindent(view: &CommandView<'_>) -> Result<(), EvalError> {
  opt_args::no_command_opt_args(view)?;
  arity::no_args(view)?;
  return Ok(());
}

/// `\pagebreak` — その位置で強制的に改ページするマーカーコマンド
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn pagebreak(view: &CommandView<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
  opt_args::no_command_opt_args(view)?;
  arity::no_args(view)?;
  return Ok(ctx.leaf_node(view.span(), HirNodeKind::PageBreak));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{evaluate_children_to_hir, run_handler, test_support};

  #[test]
  fn space_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\space[draft]{10pt}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return space(&view, ctx));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn space_reads_its_argument_as_a_length() {
    // Arrange
    let arena = Bump::new();
    let source = r"\space{5mm}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return space(&view, ctx)).unwrap();

    // Assert
    assert_eq!(result.kind, HirNodeKind::Space(Length::mm(5.0)));
  }

  #[test]
  fn space_rejects_non_length_argument() {
    // 単位のない数値が暗黙の単位（旧 pt）を持つ場所を残さない（#690）
    for source in [r"\space{5}", r"\space{5PT}", r"\space{5 pt}", r"\space{}"] {
      let arena = Bump::new();
      let node = test_support::command_call_node(source, &arena);
      let view = CommandView::new(node, source);

      let result = run_handler(|ctx| return space(&view, ctx));

      assert!(
        matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "space"),
        "{source} は拒否される: {result:?}"
      );
    }
  }

  #[test]
  fn noindent_accepts_no_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\noindent";
    let node = test_support::command_call_node(source, &arena);
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
    let node = test_support::command_call_node(source, &arena);
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
    let node = test_support::command_call_node(source, &arena);
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
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return pagebreak(&view, ctx)).unwrap();

    // Assert
    assert!(matches!(result.kind, HirNodeKind::PageBreak));
  }

  #[test]
  fn pagebreak_rejects_mandatory_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\pagebreak{x}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return pagebreak(&view, ctx));

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "pagebreak"));
  }

  #[test]
  fn pagebreak_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\pagebreak[weight=2]";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_handler(|ctx| return pagebreak(&view, ctx));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "weight"));
  }

  #[test]
  fn pagebreak_splits_surrounding_paragraph() {
    // Arrange
    let arena = Bump::new();
    let source = r"前\pagebreak 後";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 3);
    assert!(matches!(result[0].kind, HirNodeKind::Paragraph(_)));
    assert!(matches!(result[1].kind, HirNodeKind::PageBreak));
    assert!(matches!(result[2].kind, HirNodeKind::Paragraph(_)));
  }
}
