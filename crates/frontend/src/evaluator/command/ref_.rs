//! `\ref{label}` コマンド
//!
//! 必須引数 1 個（ラベル名）を取り、`InlineNode::Ref` を生成する。解決（`lowering` 層の
//! `CounterRegistry` によるラベル → 番号の解決）は行わない。

use model::InlineNode;

use crate::{
  evaluator::{EvalError, opt_args::collect_command_opt_args},
  span_ext::ToSourceSpan,
  syntax::ast::{CommandView, extract_text_content},
};

/// `\ref{label}` を `InlineNode::Ref` に変換する
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn ref_command(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
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
  return Ok(vec![InlineNode::Ref {
    label,
    span: view.span(),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    evaluator::lookup_env_parse_mode,
    syntax::{SyntaxKind, green::GreenElement},
  };

  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a crate::syntax::green::GreenNode<'a> {
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
    let result = ref_command(&view).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let InlineNode::Ref { label, .. } = &result[0] else {
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
    let result = ref_command(&view);

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
    let result = ref_command(&view);

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
    let result = ref_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
