//! `\cite{key}` / `\cite{a,b}` コマンド
//!
//! 必須引数 1 個（カンマ区切りの引用キー列）を取り、[`InlineNode::Cite`] スタブを
//! 生成する。pass1 時点では `label` は `None` で、キーの存在検証は
//! `crate::parse_source` の pass2（`cite::resolve_cites`）が行う。最終的な引用ラベルの
//! 整形は CSL 整形ステージ（`citation` クレート）に委ねる。

use model::InlineNode;

use crate::{
  evaluator::{EvalError, opt_args::collect_command_opt_args},
  syntax::ast::{CommandView, extract_text_content},
};

/// `\cite{a,b}` を `InlineNode::Cite` に変換する
///
/// 引数のテキストをカンマで分割し、各キーを trim する。空のキー（`\cite{}` や
/// `\cite{a,}` のような末尾カンマ・連続カンマ）は曖昧さを排除するためエラーとする。
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、任意引数が指定された場合、または空のキーが含まれる場合に
/// エラーを返します。
pub(crate) fn cite_command(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "cite".to_string(),
      expected: "引用キー".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "cite".to_string(),
      span: view.span().into(),
    });
  }

  let raw = extract_text_content(view.source(), first_arg);
  let segments: Vec<&str> = raw.split(',').collect();
  let mut keys = Vec::with_capacity(segments.len());
  for segment in segments {
    let key = segment.trim();
    if key.is_empty() {
      return Err(EvalError::InvalidCommandArgument {
        name: "cite".to_string(),
        reason: "空の引用キーが含まれています".to_string(),
        span: view.span().into(),
      });
    }
    keys.push(key.to_string());
  }

  return Ok(vec![InlineNode::Cite {
    keys,
    label: None,
    span: view.span().into(),
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
  fn cite_produces_single_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{rika}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = cite_command(&view).unwrap();

    // Assert
    let InlineNode::Cite { keys, label, .. } = &result[0] else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["rika".to_string()]);
    assert!(label.is_none());
  }

  #[test]
  fn cite_splits_multiple_keys_and_trims() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{a, b ,c}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = cite_command(&view).unwrap();

    // Assert
    let InlineNode::Cite { keys, .. } = &result[0] else {
      panic!("Cite が期待されます");
    };
    assert_eq!(keys, &["a".to_string(), "b".to_string(), "c".to_string()]);
  }

  #[test]
  fn cite_rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(matches!(cite_command(&view), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "cite"));
  }

  #[test]
  fn cite_rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite{a}{b}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(matches!(cite_command(&view), Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "cite"));
  }

  #[test]
  fn cite_rejects_empty_key() {
    // Arrange — 末尾カンマで空のキーが生じる
    let arena = Bump::new();
    let source = r"\cite{a,}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(matches!(cite_command(&view), Err(EvalError::InvalidCommandArgument { ref name, .. }) if name == "cite"));
  }

  #[test]
  fn cite_rejects_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\cite[k=v]{rika}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act / Assert
    assert!(matches!(cite_command(&view), Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
