//! `\ref{label}` コマンド
//!
//! 必須引数 1 個（ラベル名）を取り、pass2 で `CounterRegistry::resolve_label` により
//! 解決される [`InlineNode::Ref`] スタブを生成する。pass1 時点では `number` は `None`
//! で、`parser::parse_source` の pass2 で書き換えられる。

use syntax::ast::{CommandView, extract_text_content};

use crate::{
  document::InlineNode,
  evaluator::{EvalError, opt_args::collect_command_opt_args},
};

/// `\ref{label}` を `InlineNode::Ref` に変換する
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(super) fn ref_command(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "ref".to_string(),
      expected: "ラベル名".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "ref".to_string(),
      span: view.span().into(),
    });
  }

  let label = extract_text_content(view.source(), first_arg).trim().to_string();
  return Ok(vec![InlineNode::Ref {
    label,
    number: None,
    span: view.span().into(),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::{SyntaxKind, green::GreenElement};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a syntax::green::GreenNode<'a> {
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
  fn ref_produces_inline_ref_with_none_number() {
    // Arrange
    let arena = Bump::new();
    let source = r"\ref{sec:intro}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = ref_command(&view).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let InlineNode::Ref { label, number, .. } = &result[0] else {
      panic!("Ref が期待されます");
    };
    assert_eq!(label, "sec:intro");
    assert!(number.is_none());
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
