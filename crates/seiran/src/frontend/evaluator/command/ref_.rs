//! `\ref{label}` コマンド

use crate::{
  frontend::{
    evaluator::{EvalError, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::ast::{CommandView, extract_text_content},
  },
  model::{HirBuilder, HirInline, HirInlineKind},
};

/// `\ref{label}` を `HirInlineKind::Ref` に変換する
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn ref_command(view: &CommandView, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "ref".to_string(),
      expected: "ラベル名".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "ref".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let label = extract_text_content(view.source(), first_arg).trim().to_string();
  return Ok(vec![builder.leaf_inline(view.span(), HirInlineKind::Ref { label })]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::{
    evaluator::{lookup_env_parse_mode, run_inline_handler},
    syntax::{SyntaxKind, green::GreenElement},
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
  fn ref_produces_inline_ref_stub() {
    // Arrange
    let arena = Bump::new();
    let source = r"\ref{sec:intro}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return ref_command(&view, builder)).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirInlineKind::Ref { label } = &result[0].kind else {
      panic!("Ref が期待されます");
    };
    assert_eq!(label, "sec:intro");
  }

  #[test]
  fn ref_rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\ref";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return ref_command(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "ref"));
  }

  #[test]
  fn ref_rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\ref{a}{b}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return ref_command(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "ref"));
  }

  #[test]
  fn ref_rejects_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\ref[k=v]{label}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_inline_handler(|builder| return ref_command(&view, builder));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
