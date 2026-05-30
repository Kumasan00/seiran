//! 見出しコマンド群
//!
//! `\part`, `\chapter`, `\section`, `\subsection`, `\paragraph`, `\subparagraph`
//! コマンドの実装です。各コマンドは見出しレベルに応じた `DocNode::Heading` を生成し、
//! 自動採番を行います。

use syntax::ast::CommandView;

use crate::{
  document::{DocNode, HeadingLevel, HeadingNumber},
  evaluator::{EvalContext, EvalError, inline::extract_inline_nodes, opt_args::collect_command_opt_args},
};

/// 見出しコマンドの共通処理
///
/// カウンタのインクリメント・リセット、引数バリデーション、
/// `DocNode::Heading` の生成をすべて行います。
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
/// * `level` - 見出しレベル
/// * `context` - 評価コンテキスト（採番用）
///
/// # Errors
///
/// 引数不足・過剰の場合にエラーを返します
pub(super) fn heading(
  view: &CommandView,
  level: HeadingLevel,
  context: &mut EvalContext,
) -> Result<Vec<DocNode>, EvalError> {
  let name = level.command_name();

  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: level.expected_name().to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().into(),
    });
  }

  context.increment_heading(level);
  let title = extract_inline_nodes(view.source(), first_arg);
  let number = HeadingNumber::from_context(level, context);

  return Ok(vec![DocNode::Heading {
    level,
    number,
    title,
    label: None,
  }]);
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
  fn heading_rejects_unknown_opt_arg_key() {
    // Arrange — 見出しは現状任意引数を受け付けないので `[label=...]` は不明キー
    let arena = Bump::new();
    let source = r"\section[label=foo]{Title}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);
    let mut context = EvalContext::default();

    // Act
    let result = heading(&view, HeadingLevel::Section, &mut context);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }
}
