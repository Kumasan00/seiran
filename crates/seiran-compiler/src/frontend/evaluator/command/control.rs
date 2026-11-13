//! スペースや改ページなどの制御コマンド群

use crate::{
  document::{HirNode, HirNodeKind},
  frontend::{
    evaluator::{EvalContext, EvalError, opt_args, signature::Signature, string_arg},
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\space{<長さ>}` の署名
pub(super) const SPACE: Signature<1> = Signature::new([ArgMode::Inherit], "スペース量（長さ）", &[]);

/// `\space{<長さ>}` — 固定幅スペースを挿入するコマンド
///
/// 長さの書式は config / style と同じ（[`crate::Length`] の `FromStr`）。表せる長さなら値域は制限しない（負値は詰め）。
///
/// # Errors
///
/// 引数が字以外の要素（コマンド・数式など）を含む・長さとして読めない場合にエラーを返します
pub(super) fn space(view: &CommandView<'_>, arg: &CstNode<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
  let text = string_arg::text(view, arg)?;

  let length = opt_args::parse_length(&text, view.span().into(), || {
    return EvalError::InvalidCommandArgument {
      name: "space".to_string(),
      reason: "長さ（`<数値>pt` / `<数値>mm` / `<数値>cm`）".to_string(),
      span: view.span().into(),
    };
  })?;

  return Ok(ctx.leaf_node(view.span(), HirNodeKind::Space(length)));
}

/// `\pagebreak` — その位置で強制的に改ページするマーカーコマンド
pub(super) fn pagebreak(view: &CommandView<'_>, ctx: &EvalContext<'_>) -> HirNode {
  return ctx.leaf_node(view.span(), HirNodeKind::PageBreak);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::evaluator::{evaluate_children_to_hir, run_handler, test_support},
    length::Length,
  };

  #[test]
  fn space_reads_its_argument_as_a_length() {
    let arena = Bump::new();
    let source = r"\space{5mm}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let [arg] = SPACE.bind(&view).unwrap().args;

    let result = run_handler(|ctx| return space(&view, arg, ctx)).unwrap();

    assert_eq!(result.kind, HirNodeKind::Space(Length::mm(5.0)));
  }

  #[test]
  fn space_rejects_non_length_argument() {
    // 単位のない数値が暗黙の単位を持つ場所を残さない
    for source in [r"\space{5}", r"\space{5PT}", r"\space{5 pt}", r"\space{}"] {
      let arena = Bump::new();
      let node = test_support::command_call_node(source, &arena);
      let view = CommandView::new(node, source);

      let [arg] = SPACE.bind(&view).unwrap().args;

      let result = run_handler(|ctx| return space(&view, arg, ctx));

      assert!(
        matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "space"),
        "{source} は拒否される: {result:?}"
      );
    }
  }

  #[test]
  fn pagebreak_splits_surrounding_paragraph() {
    let arena = Bump::new();
    let source = r"前\pagebreak 後";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 3);
    assert!(matches!(result[0].kind, HirNodeKind::Paragraph(_)));
    assert!(matches!(result[1].kind, HirNodeKind::PageBreak));
    assert!(matches!(result[2].kind, HirNodeKind::Paragraph(_)));
  }
}
