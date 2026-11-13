//! 書体・文字色を指定するコマンド群

use crate::{
  color::Color,
  document::{HirInline, HirInlineKind, Typeface},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{IndexPolicy, evaluate_inline_children},
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::Signature,
    },
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\color[color=#rrggbb]`（文字色）
const COLOR: OptKey<Color> = opt_args::color("color");
/// `\color` が受け付けるキー
const COLOR_KEYS: &[OptDecl] = &[COLOR.decl()];

/// 書体コマンド（`\bold` 等）の署名
pub(super) const STYLED: Signature<1> = Signature::new([ArgMode::Inherit], "テキスト", &[]);
/// `\color` の署名
pub(super) const COLORED: Signature<1> = Signature::new([ArgMode::Inherit], "テキスト", COLOR_KEYS);

/// 引数 1 つを取り、子要素を `HirInline` リストに変換して `HirInlineKind::Styled` でラップする共通処理
///
/// # Errors
///
/// 引数内の評価エラーをそのまま伝播します。
pub(super) fn styled_text(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  ctx: &EvalContext<'_>,
  font: Typeface,
  index_policy: IndexPolicy,
) -> Result<HirInline, EvalError> {
  let id = ctx.alloc(view.span());
  let children = evaluate_inline_children(view.source(), ctx, arg, index_policy)?;
  return Ok(HirInline::new(id, HirInlineKind::Styled { font, children }));
}

/// `\color[color=#rrggbb]{...}` を評価し、子要素を `HirInlineKind::Colored` でラップする
///
/// # Errors
///
/// 色の欠落で [`EvalError::MissingCommandArgument`] を返します。引数内の評価エラーはそのまま伝播します。
pub(super) fn colored_text(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  opts: &OptArgs,
  ctx: &EvalContext<'_>,
  index_policy: IndexPolicy,
) -> Result<HirInline, EvalError> {
  let Some(color) = opts.get(COLOR) else {
    return Err(EvalError::MissingCommandArgument {
      name: view.name().to_string(),
      expected: "色 (color=#rrggbb)".to_string(),
      span: view.span().into(),
    });
  };

  let id = ctx.alloc(view.span());
  let children = evaluate_inline_children(view.source(), ctx, arg, index_policy)?;
  return Ok(HirInline::new(id, HirInlineKind::Colored { color, children }));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_handler, test_support};

  #[test]
  fn bold_creates_styled_node() {
    let arena = Bump::new();
    let source = "\\bold{hello}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = STYLED.bind(&view)?;
      return styled_text(&view, bound.args[0], ctx, Typeface::SerifBold, IndexPolicy::Allow);
    })
    .unwrap();

    match &result.kind {
      HirInlineKind::Styled { font, children } => {
        assert_eq!(*font, Typeface::SerifBold);
        assert_eq!(children.len(), 1);
        assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "hello"));
      },
      _ => panic!("Styled が期待されます"),
    }
  }

  #[test]
  fn nested_styled_commands_keep_inner_font() {
    let arena = Bump::new();
    let source = r"\bold{\italic{x}}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = STYLED.bind(&view)?;
      return styled_text(&view, bound.args[0], ctx, Typeface::SerifBold, IndexPolicy::Allow);
    })
    .unwrap();

    let HirInlineKind::Styled { font, children } = &result.kind else {
      panic!("Styled が期待されます");
    };
    assert_eq!(*font, Typeface::SerifBold);
    let HirInlineKind::Styled {
      font: inner_font, ..
    } = &children[0].kind
    else {
      panic!("内側も Styled が期待されます: {children:?}");
    };
    assert_eq!(*inner_font, Typeface::SerifItalic);
  }

  #[test]
  fn color_creates_colored_node() {
    let arena = Bump::new();
    let source = r"\color[color=#ff0000]{x}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = COLORED.bind(&view)?;
      return colored_text(&view, bound.args[0], &bound.opts, ctx, IndexPolicy::Allow);
    })
    .unwrap();

    let HirInlineKind::Colored { color, children } = &result.kind else {
      panic!("Colored が期待されます: {result:?}");
    };
    assert_eq!(*color, Color::new(0xff, 0x00, 0x00));
    assert_eq!(children.len(), 1);
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "x"));
  }

  #[test]
  fn color_rejects_missing_color() {
    let arena = Bump::new();
    let source = r"\color{x}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(matches!(
      run_handler(|ctx| {
        let bound = COLORED.bind(&view)?;
        return colored_text(&view, bound.args[0], &bound.opts, ctx, IndexPolicy::Allow);
      }),
      Err(EvalError::MissingCommandArgument { .. })
    ));
  }

  #[test]
  fn color_rejects_invalid_hex() {
    let arena = Bump::new();
    let source = r"\color[color=#zzzzzz]{x}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    assert!(matches!(run_handler(|ctx| {
      let bound = COLORED.bind(&view)?;
      return colored_text(&view, bound.args[0], &bound.opts, ctx, IndexPolicy::Allow);
    }), Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "color"));
  }

  #[test]
  fn nested_bold_inside_color_keeps_both() {
    let arena = Bump::new();
    let source = r"\color[color=#0000ff]{\bold{x}}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = run_handler(|ctx| {
      let bound = COLORED.bind(&view)?;
      return colored_text(&view, bound.args[0], &bound.opts, ctx, IndexPolicy::Allow);
    })
    .unwrap();

    let HirInlineKind::Colored { color, children } = &result.kind else {
      panic!("Colored が期待されます: {result:?}");
    };
    assert_eq!(*color, Color::new(0x00, 0x00, 0xff));
    let HirInlineKind::Styled { font, .. } = &children[0].kind else {
      panic!("内側は Styled が期待されます: {children:?}");
    };
    assert_eq!(*font, Typeface::SerifBold);
  }
}
