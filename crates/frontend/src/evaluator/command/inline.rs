//! 書体指定コマンド群
//!
//! `\bold`, `\italic`, `\sans`, `\monobold` など、テキストに書体（ファミリ × スタイル）を
//! 適用するインラインコマンドの共通処理を提供します。
//! 各コマンドは子要素を再帰的に `InlineNode` に変換し、対応する `FontKind` を持つ
//! [`InlineNode::Styled`] を生成します。ネスト時は内側の書体が完全に上書きします。

use model::{FontKind, InlineNode};

use crate::{
  evaluator::{
    EvalError,
    inline::extract_inline_nodes,
    opt_args::{OptType, collect_command_opt_args, find_color},
  },
  span_ext::ToSourceSpan,
  syntax::ast::CommandView,
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
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().to_source_span(),
    });
  }

  let children = extract_inline_nodes(view.source(), first_arg)?;
  return Ok(vec![InlineNode::Styled { kind, children }]);
}

/// `\color[color=#rrggbb]{...}` を評価し、子要素を [`InlineNode::Colored`] でラップする
///
/// 色は任意引数 `color`（`#rrggbb`）で必須指定する。原則 A（引数は `{}` で明示）に従い
/// 色は opt arg、着色対象は必須引数 1 つで受ける。`CommandKind::ColoredText` から呼ばれます。
///
/// # Errors
///
/// 色の欠落・必須引数の不足で [`EvalError::MissingCommandArgument`]、引数過剰で
/// [`EvalError::ExtraCommandArgument`]、色の 16 進表記が不正な場合に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn colored_text(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let name = view.name();
  let opt_args = collect_command_opt_args(view, &[("color", OptType::Color)])?;
  let Some(color) = find_color(&opt_args, "color") else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: "色 (color=#rrggbb)".to_string(),
      span: view.span().to_source_span(),
    });
  };
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: "テキスト".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().to_source_span(),
    });
  }

  let children = extract_inline_nodes(view.source(), first_arg)?;
  return Ok(vec![InlineNode::Colored { color, children }]);
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

  /// テスト用: ソースからコマンドビューを取得
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

  #[test]
  fn color_creates_colored_node() {
    // Arrange
    let arena = Bump::new();
    let source = r"\color[color=#ff0000]{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = colored_text(&view).unwrap();

    // Assert — 指定色の Colored ノードが生成され、子はそのテキスト
    assert_eq!(result.len(), 1);
    let InlineNode::Colored { color, children } = &result[0] else {
      panic!("Colored が期待されます: {result:?}");
    };
    assert_eq!(*color, model::Color::new(0xff, 0x00, 0x00));
    assert_eq!(children.len(), 1);
    assert!(matches!(&children[0], InlineNode::Text(t) if t == "x"));
  }

  #[test]
  fn color_rejects_missing_color() {
    // Arrange — `color` 任意引数を欠くと色を確定できない
    let arena = Bump::new();
    let source = r"\color{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert
    assert!(matches!(colored_text(&view), Err(EvalError::MissingCommandArgument { .. })));
  }

  #[test]
  fn color_rejects_invalid_hex() {
    // Arrange — `#zzzzzz` は 16 進表記として不正
    let arena = Bump::new();
    let source = r"\color[color=#zzzzzz]{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert — opt arg 解析段で型エラーになる
    assert!(matches!(colored_text(&view), Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "color"));
  }

  #[test]
  fn color_rejects_extra_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\color[color=#00ff00]{a}{b}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act & Assert
    assert!(matches!(colored_text(&view), Err(EvalError::ExtraCommandArgument { .. })));
  }

  #[test]
  fn nested_bold_inside_color_keeps_both() {
    // Arrange — \color[...]{\bold{x}} は色（Colored）の中に書体（Styled）が保持される
    let arena = Bump::new();
    let source = r"\color[color=#0000ff]{\bold{x}}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = colored_text(&view).unwrap();

    // Assert — 外側 Colored の子として内側 Styled(SerifBold) がそのまま保持される
    let InlineNode::Colored { color, children } = &result[0] else {
      panic!("Colored が期待されます: {result:?}");
    };
    assert_eq!(*color, model::Color::new(0x00, 0x00, 0xff));
    let InlineNode::Styled { kind, .. } = &children[0] else {
      panic!("内側は Styled が期待されます: {children:?}");
    };
    assert_eq!(*kind, FontKind::SerifBold);
  }
}
