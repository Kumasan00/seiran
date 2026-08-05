//! `\footnote{...}` コマンド

use crate::{
  frontend::{
    evaluator::{EvalError, inline::extract_inline_nodes, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::ast::CommandView,
  },
  model::{HirBuilder, HirInline, HirInlineKind},
};

/// `\footnote{...}` を `InlineNode::Footnote` に変換する
///
/// 本体は太字・数式等を含む任意のインライン内容として再帰評価される。
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn footnote_command(view: &CommandView, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
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
  let body = extract_inline_nodes(view.source(), builder, first_arg)?;
  return Ok(vec![HirInline::new(id, HirInlineKind::Footnote { body })]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::{
      evaluator::{lookup_env_parse_mode, run_inline_handler},
      syntax::{SyntaxKind, green::GreenElement},
    },
    model::InlineNode,
  };

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
  fn footnote_with_plain_text_produces_footnote_node() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote{hello}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder)).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let InlineNode::Footnote { body, .. } = &result[0] else {
      panic!("Footnote が期待されます");
    };
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0], InlineNode::Text(t) if t == "hello"));
  }

  #[test]
  fn footnote_with_styled_body_recursively_evaluates() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote{\bold{x}}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder)).unwrap();

    // Assert
    let InlineNode::Footnote { body, .. } = &result[0] else {
      panic!("Footnote が期待されます");
    };
    assert_eq!(body.len(), 1);
    let InlineNode::Styled { kind, children } = &body[0] else {
      panic!("Styled が期待されます: {body:?}");
    };
    assert_eq!(*kind, crate::model::FontKind::SerifBold);
    assert!(matches!(&children[0], InlineNode::Text(t) if t == "x"));
  }

  #[test]
  fn footnote_rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "footnote"));
  }

  #[test]
  fn footnote_rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote{a}{b}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "footnote"));
  }

  #[test]
  fn footnote_rejects_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\footnote[k=v]{body}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return footnote_command(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
