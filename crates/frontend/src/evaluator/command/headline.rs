//! 見出しコマンド群

use model::{DocNode, HeadingLevel};

use crate::{
  evaluator::{
    EvalError,
    inline::extract_inline_nodes,
    opt_args::{OptType, collect_command_opt_args, find_string},
  },
  span_ext::ToSourceSpan,
  syntax::ast::CommandView,
};

/// 見出しコマンドの共通処理
///
/// ラベルとタイトルだけを構造化し、採番は後段に委ねる。
///
/// # Errors
///
/// 引数不足・過剰、または `[label=...]` の値型不一致でエラーを返します。
pub(super) fn heading(view: &CommandView, level: HeadingLevel) -> Result<Vec<DocNode>, EvalError> {
  let name = level.command_name();

  let opt_args = collect_command_opt_args(view, &[("label", OptType::String)])?;
  let label = find_string(&opt_args, "label");

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: expected_name(level).to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().to_source_span(),
    });
  }

  let title = extract_inline_nodes(view.source(), first_arg)?;

  return Ok(vec![DocNode::Heading {
    level,
    numbered: true,
    title,
    label,
    span: view.span(),
  }]);
}

/// `HeadingLevel` のエラーメッセージ用引数説明を返すヘルパー
fn expected_name(level: HeadingLevel) -> &'static str {
  return match level {
    HeadingLevel::Part => "部名",
    HeadingLevel::Chapter => "章名",
    HeadingLevel::Section => "セクション名",
    HeadingLevel::Subsection => "サブセクション名",
    HeadingLevel::Paragraph => "段落名",
    HeadingLevel::Subparagraph => "小節名",
  };
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

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
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
  fn heading_captures_label_and_is_numbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[label=sec:foo]{Title}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = heading(&view, HeadingLevel::Section).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let DocNode::Heading {
      level,
      numbered,
      label,
      ..
    } = &result[0]
    else {
      panic!("Heading が期待されます");
    };
    assert_eq!(*level, HeadingLevel::Section);
    assert!(*numbered);
    assert_eq!(label.as_deref(), Some("sec:foo"));
  }

  #[test]
  fn heading_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[draft=true]{Title}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = heading(&view, HeadingLevel::Section);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }
}
