//! 書体指定コマンド群
//!
//! `\bold`, `\italic`, `\sans`, `\monobold` など、テキストに書体（ファミリ × スタイル）を
//! 適用するインラインコマンドの共通処理を提供します。
//! 各コマンドは子要素を再帰的に `InlineNode` に変換し、対応する `FontKind` を持つ
//! [`InlineNode::Styled`] を生成します。ネスト時は内側の書体が完全に上書きします。

use syntax::ast::CommandView;
use types::FontKind;

use crate::{
  document::InlineNode,
  evaluator::{EvalError, inline::extract_inline_nodes, opt_args::collect_command_opt_args},
};

/// 引数 1 つを取り、子要素を `InlineNode` リストに変換して [`InlineNode::Styled`] でラップする共通処理
///
/// `CommandKind::StyledText` から呼ばれます。
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
/// * `kind` - 適用する書体
///
/// # Errors
///
/// 引数の不足・過剰の場合にエラーを返します
pub(crate) fn styled_text(view: &CommandView, kind: FontKind) -> Result<Vec<InlineNode>, EvalError> {
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

  let children = extract_inline_nodes(view.source(), first_arg)?;
  return Ok(vec![InlineNode::Styled { kind, children }]);
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
  fn bold_creates_styled_node() {
    // Arrange
    let arena = Bump::new();
    let source = "\\bold{hello}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = styled_text(&view, FontKind::SerifBold).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      InlineNode::Styled { kind, children } => {
        assert_eq!(*kind, FontKind::SerifBold);
        assert_eq!(children.len(), 1);
        assert!(matches!(&children[0], InlineNode::Text(t) if t == "hello"));
      },
      _ => panic!("Styled が期待されます"),
    }
  }

  #[test]
  fn nested_styled_commands_keep_inner_kind() {
    // Arrange — \bold{\italic{x}} のネストは内側が完全上書き（合成しない）
    let arena = Bump::new();
    let source = r"\bold{\italic{x}}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = styled_text(&view, FontKind::SerifBold).unwrap();

    // Assert — 外側 SerifBold の子として内側 SerifItalic がそのまま保持される
    let InlineNode::Styled { kind, children } = &result[0] else {
      panic!("Styled が期待されます");
    };
    assert_eq!(*kind, FontKind::SerifBold);
    let InlineNode::Styled {
      kind: inner_kind, ..
    } = &children[0]
    else {
      panic!("内側も Styled が期待されます: {children:?}");
    };
    assert_eq!(*inner_kind, FontKind::SerifItalic);
  }

  #[test]
  fn rejects_missing_argument() {
    // Arrange
    let arena = Bump::new();
    let source = "\\bold";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert
    assert!(matches!(styled_text(&view, FontKind::SerifBold), Err(EvalError::MissingCommandArgument { .. })));
  }

  #[test]
  fn rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = "\\bold{a}{b}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert
    assert!(matches!(styled_text(&view, FontKind::SerifBold), Err(EvalError::ExtraCommandArgument { .. })));
  }

  #[test]
  fn bold_rejects_unknown_opt_arg_key() {
    // Arrange — 書体指定コマンドは任意引数を受け付けないので `[heavy]` は不明キー
    let arena = Bump::new();
    let source = r"\bold[heavy]{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = styled_text(&view, FontKind::SerifBold);

    // Assert — boolean ショートハンドの `heavy` も未許可キーとして拒否される
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "heavy"));
  }
}
