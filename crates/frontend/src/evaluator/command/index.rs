//! `\index{語}` コマンド
//!
//! 必須引数 1 個（索引語、プレーンテキストのみ）とオプション `[reading=...]` を取り、
//! `InlineNode::Index` を生成する。本文に出力を持たない索引収集用マーカー。

use model::InlineNode;

use crate::{
  evaluator::{
    EvalError,
    inline::extract_inline_nodes,
    opt_args::{OptType, collect_command_opt_args, find_string},
  },
  span_ext::ToSourceSpan,
  syntax::ast::CommandView,
};

/// `\index{語}` / `\index[reading=よみ]{語}` を `InlineNode::Index` に変換する
///
/// # Errors
///
/// 必須引数の欠落・過剰、未知の任意引数キー、語が非プレーンテキスト（インライン装飾・数式・
/// コマンドを含む）または空文字列の場合にエラーを返します。
pub(crate) fn index_command(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("reading", OptType::String)])?;
  let reading = find_string(&opt_args, "reading");

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "index".to_string(),
      expected: "索引語".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "index".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let nodes = extract_inline_nodes(view.source(), first_arg)?;
  let mut word = String::new();
  for node in &nodes {
    let InlineNode::Text(text) = node else {
      return Err(EvalError::InvalidCommandArgument {
        name: "index".to_string(),
        reason: "索引語はプレーンテキストのみ使用できます（インライン装飾・数式・コマンドは不可）".to_string(),
        span: view.span().to_source_span(),
      });
    };
    word.push_str(text);
  }
  let word = word.trim().to_string();
  if word.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "index".to_string(),
      reason: "索引語を空にはできません".to_string(),
      span: view.span().to_source_span(),
    });
  }

  return Ok(vec![InlineNode::Index {
    word,
    reading: reading.map(|r| return r.trim().to_string()),
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
  fn index_produces_inline_index_stub() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index{語}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let InlineNode::Index { word, reading, .. } = &result[0] else {
      panic!("Index が期待されます");
    };
    assert_eq!(word, "語");
    assert_eq!(*reading, None);
  }

  #[test]
  fn index_with_reading_opt_arg() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index[reading=よみ]{語}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view).unwrap();

    // Assert
    let InlineNode::Index { reading, .. } = &result[0] else {
      panic!("Index が期待されます");
    };
    assert_eq!(reading.as_deref(), Some("よみ"));
  }

  #[test]
  fn index_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index[foo=bar]{語}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn index_rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_extra_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index{語}{余分}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_empty_word() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index{ }";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_styled_text_inside_word() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index{\bold{語}}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_symbol_inside_word() {
    // Arrange — 単一文字コマンド（`\alpha`）も Text ではないため拒否される
    let arena = Bump::new();
    let source = r"\index{\alpha}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_inline_math_inside_word() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index{$x$}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_rejects_command_inside_word() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index{\ref{sec:x}}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "index"));
  }

  #[test]
  fn index_trims_word_and_reading() {
    // Arrange
    let arena = Bump::new();
    let source = r"\index[reading= よみ ]{ 語 }";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = index_command(&view).unwrap();

    // Assert
    let InlineNode::Index { word, reading, .. } = &result[0] else {
      panic!("Index が期待されます");
    };
    assert_eq!(word, "語");
    assert_eq!(reading.as_deref(), Some("よみ"));
  }
}
