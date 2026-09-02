//! `\footnote{...}` コマンド

use crate::{
  document::{HirBuilder, HirInline, HirInlineKind},
  frontend::{
    evaluator::{
      EvalError,
      inline::{IndexPolicy, extract_inline_nodes},
      opt_args::collect_command_opt_args,
    },
    span_ext::ToSourceSpan,
    syntax::view::CommandView,
  },
};

/// `\footnote{...}` を `HirInlineKind::Footnote` に変換する
///
/// 本体は太字・数式等を含む任意のインライン内容として再帰評価される。`\index` の可否は
/// `index_policy` をそのまま引き継ぐ — 脚注本体は 1 箇所にしか置かれないが、複製される文脈
/// （表の `\head` セル）や本文の流れに出ない文脈（見出しタイトル）の中の脚注まで許すと、
/// その文脈を拒否している意味が失われるため。
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn footnote_command(
  view: &CommandView<'_>,
  builder: &HirBuilder,
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "footnote".to_string(),
      expected: "脚注本体".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "footnote".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let id = builder.alloc(view.span());
  let body = extract_inline_nodes(view.source(), builder, first_arg, index_policy)?;
  return Ok(vec![HirInline::new(id, HirInlineKind::Footnote { body })]);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::FontKind,
    frontend::evaluator::{run_inline_handler, test_support},
  };

  #[test]
  fn footnote_with_plain_text_produces_footnote_node() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote{hello}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder, IndexPolicy::Allow)).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirInlineKind::Footnote { body } = &result[0].kind else {
      panic!("Footnote が期待されます");
    };
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0].kind, HirInlineKind::Text(t) if t == "hello"));
  }

  #[test]
  fn footnote_with_styled_body_recursively_evaluates() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote{\bold{x}}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder, IndexPolicy::Allow)).unwrap();

    // Assert
    let HirInlineKind::Footnote { body } = &result[0].kind else {
      panic!("Footnote が期待されます");
    };
    assert_eq!(body.len(), 1);
    let HirInlineKind::Styled { kind, children } = &body[0].kind else {
      panic!("Styled が期待されます: {body:?}");
    };
    assert_eq!(*kind, FontKind::SerifBold);
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "x"));
  }

  #[test]
  fn footnote_rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder, IndexPolicy::Allow));

    // Assert
    assert!(matches!(result, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "footnote"));
  }

  #[test]
  fn footnote_rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote{a}{b}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder, IndexPolicy::Allow));

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "footnote"));
  }

  #[test]
  fn footnote_rejects_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote[k=v]{body}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder, IndexPolicy::Allow));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
