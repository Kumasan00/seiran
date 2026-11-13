//! `\footnote{...}` コマンド

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{IndexPolicy, evaluate_inline_children},
      signature::Signature,
    },
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\footnote` の署名
pub(super) const SIGNATURE: Signature<1> = Signature::new([ArgMode::Inherit], "脚注本体", &[]);

/// `\footnote{...}` を `HirInlineKind::Footnote` に変換する
///
/// 本体は太字・数式等を含む任意のインライン内容として再帰評価される。`\index` の可否は
/// `index_policy` をそのまま引き継ぐ — 脚注本体は 1 箇所にしか置かれないが、複製される文脈
/// （表の `\head` セル）や本文の流れに出ない文脈（見出しタイトル）の中の脚注まで許すと、
/// その文脈を拒否している意味が失われるため。
///
/// # Errors
///
/// 本体の評価エラーを返します。
pub(super) fn footnote(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  ctx: &EvalContext<'_>,
  index_policy: IndexPolicy,
) -> Result<HirInline, EvalError> {
  let id = ctx.alloc(view.span());
  let body = evaluate_inline_children(view.source(), ctx, arg, index_policy)?;
  return Ok(HirInline::new(id, HirInlineKind::Footnote { body }));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::Typeface,
    frontend::evaluator::{run_handler, test_support},
  };

  #[test]
  fn footnote_with_plain_text_produces_footnote_node() {
    let arena = Bump::new();
    let source = r"\footnote{hello}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result =
      run_handler(|ctx| return footnote(&view, SIGNATURE.bind(&view)?.args[0], ctx, IndexPolicy::Allow)).unwrap();

    let HirInlineKind::Footnote { body } = &result.kind else {
      panic!("Footnote が期待されます");
    };
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0].kind, HirInlineKind::Text(t) if t == "hello"));
  }

  #[test]
  fn footnote_with_styled_body_recursively_evaluates() {
    let arena = Bump::new();
    let source = r"\footnote{\bold{x}}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result =
      run_handler(|ctx| return footnote(&view, SIGNATURE.bind(&view)?.args[0], ctx, IndexPolicy::Allow)).unwrap();

    let HirInlineKind::Footnote { body } = &result.kind else {
      panic!("Footnote が期待されます");
    };
    assert_eq!(body.len(), 1);
    let HirInlineKind::Styled { font, children } = &body[0].kind else {
      panic!("Styled が期待されます: {body:?}");
    };
    assert_eq!(*font, Typeface::SerifBold);
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "x"));
  }
}
