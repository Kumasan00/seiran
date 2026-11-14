//! `\index{語}` コマンド

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{InlinePolicy, evaluate_inline_children},
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::Signature,
    },
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\index[reading=...]`（読み仮名の上書き）
const READING: OptKey<String> = opt_args::string("reading");
/// `\index` が受け付けるキー
const KEYS: &[OptDecl] = &[READING.decl()];

/// `\index` の署名
pub(super) const SIGNATURE: Signature<1> = Signature::new([ArgMode::Inherit], "索引語", KEYS);

/// `\index{語}` / `\index[reading=よみ]{語}` を `HirInlineKind::Index` に変換する
///
/// # Errors
///
/// 語が非プレーンテキスト（インライン装飾・数式・コマンドを含む）または空文字列の場合にエラーを返します。
pub(super) fn index(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  opts: &OptArgs,
  ctx: &EvalContext<'_>,
) -> Result<HirInline, EvalError> {
  let reading = opts.get(READING);

  let nodes = evaluate_inline_children(view.source(), ctx, arg, InlinePolicy::RejectIndex)?;
  let mut word = String::new();
  for node in &nodes {
    let HirInlineKind::Text(text) = &node.kind else {
      return Err(EvalError::InvalidCommandArgument {
        name: "index".to_string(),
        reason: "索引語はプレーンテキストのみ使用できます（インライン装飾・数式・コマンドは不可）".to_string(),
        span: view.span().into(),
      });
    };
    word.push_str(text);
  }
  let word = word.trim().to_string();
  if word.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "index".to_string(),
      reason: "索引語を空にはできません".to_string(),
      span: view.span().into(),
    });
  }

  return Ok(ctx.leaf_inline(
    view.span(),
    HirInlineKind::Index {
      word,
      reading: reading.map(|r| return r.trim().to_string()),
    },
  ));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_handler, test_support};

  #[test]
  fn index_produces_inline_index_stub() {
    let arena = Bump::new();
    let source = r"\index{語}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = SIGNATURE.bind(&view)?;
      return index(&view, bound.args[0], &bound.opts, ctx);
    })
    .unwrap();

    let HirInlineKind::Index { word, reading } = &result.kind else {
      panic!("Index が期待されます");
    };
    assert_eq!(word, "語");
    assert_eq!(*reading, None);
  }

  #[test]
  fn index_with_reading_opt_arg() {
    let arena = Bump::new();
    let source = r"\index[reading=よみ]{語}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = SIGNATURE.bind(&view)?;
      return index(&view, bound.args[0], &bound.opts, ctx);
    })
    .unwrap();

    let HirInlineKind::Index { reading, .. } = &result.kind else {
      panic!("Index が期待されます");
    };
    assert_eq!(reading.as_deref(), Some("よみ"));
  }

  #[test]
  fn index_rejects_empty_word() {
    let arena = Bump::new();
    let source = r"\index{ }";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = SIGNATURE.bind(&view)?;
      return index(&view, bound.args[0], &bound.opts, ctx);
    });

    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_styled_text_inside_word() {
    let arena = Bump::new();
    let source = r"\index{\bold{語}}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = SIGNATURE.bind(&view)?;
      return index(&view, bound.args[0], &bound.opts, ctx);
    });

    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_trims_word_and_reading() {
    let arena = Bump::new();
    let source = r"\index[reading= よみ ]{ 語 }";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = SIGNATURE.bind(&view)?;
      return index(&view, bound.args[0], &bound.opts, ctx);
    })
    .unwrap();

    let HirInlineKind::Index { word, reading } = &result.kind else {
      panic!("Index が期待されます");
    };
    assert_eq!(word, "語");
    assert_eq!(reading.as_deref(), Some("よみ"));
  }
}
