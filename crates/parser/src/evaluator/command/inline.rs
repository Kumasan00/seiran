//! インラインテキスト装飾コマンド群
//!
//! `\textbf`, `\emph`, `\textit`, `\texttt`, `\textsf` など、
//! テキストにスタイルを適用するインラインコマンドの共通処理を提供します。
//! 各コマンドは子要素を再帰的に `InlineNode` に変換し、
//! 対応するラッパーノードを生成します。

use syntax::ast::CommandView;

use crate::{
  document::InlineNode,
  evaluator::{EvalError, inline::extract_inline_nodes, opt_args::collect_command_opt_args},
};

/// 引数1つを取り、子要素を `InlineNode` リストに変換してラップする共通処理
///
/// `CommandKind::InlineWrapper` から呼ばれます。
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
/// * `wrapper` - `Vec<InlineNode>` を受け取りラッパー `InlineNode` を生成する関数
///
/// # Errors
///
/// 引数の不足・過剰の場合にエラーを返します
pub(super) fn inline_wrapper(
  view: &CommandView,
  wrapper: fn(Vec<InlineNode>) -> InlineNode,
) -> Result<Vec<InlineNode>, EvalError> {
  let name = view.name();
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: "テキスト".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().into(),
    });
  }

  let children = extract_inline_nodes(view.source(), first_arg);
  return Ok(vec![wrapper(children)]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::{SyntaxKind, green::GreenElement};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// テスト用: ソースからコマンドビューを取得
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
  fn textbf_creates_strong_node() {
    // Arrange
    let arena = Bump::new();
    let source = "\\textbf{hello}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::Strong).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      InlineNode::Strong(children) => {
        assert_eq!(children.len(), 1);
        assert!(matches!(&children[0], InlineNode::Text(t) if t == "hello"));
      },
      _ => panic!("Strong が期待されます"),
    }
  }

  #[test]
  fn emph_creates_emphasis_node() {
    // Arrange
    let arena = Bump::new();
    let source = "\\emph{world}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::Emphasis).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], InlineNode::Emphasis(_)));
  }

  #[test]
  fn textit_creates_emphasis_node() {
    // Arrange
    let arena = Bump::new();
    let source = "\\textit{italic}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::Emphasis).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], InlineNode::Emphasis(_)));
  }

  #[test]
  fn texttt_creates_code_node() {
    // Arrange
    let arena = Bump::new();
    let source = "\\texttt{code}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::Code).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], InlineNode::Code(_)));
  }

  #[test]
  fn textsf_creates_sans_serif_node() {
    // Arrange
    let arena = Bump::new();
    let source = "\\textsf{sans}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::SansSerif).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], InlineNode::SansSerif(_)));
  }

  #[test]
  fn rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = "\\textbf";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert
    assert!(matches!(inline_wrapper(&view, InlineNode::Strong), Err(EvalError::MissingCommandArgument { .. })));
  }

  #[test]
  fn rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = "\\textbf{a}{b}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert
    assert!(matches!(inline_wrapper(&view, InlineNode::Strong), Err(EvalError::ExtraCommandArgument { .. })));
  }

  #[test]
  fn textbf_rejects_unknown_opt_arg_key() {
    // Arrange — インライン装飾は任意引数を受け付けないので `[bold]` は不明キー
    let arena = Bump::new();
    let source = r"\textbf[bold]{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::Strong);

    // Assert — boolean ショートハンドの `bold` も未許可キーとして拒否される
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "bold"));
  }

  #[test]
  fn nested_inline_commands() {
    // Arrange — \textbf{\emph{nested}} のような入れ子
    let arena = Bump::new();
    let source = "\\textbf{hello}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = inline_wrapper(&view, InlineNode::Strong).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    if let InlineNode::Strong(children) = &result[0] {
      assert!(!children.is_empty());
    } else {
      panic!("Strong が期待されます");
    }
  }
}
