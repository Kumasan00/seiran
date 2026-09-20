//! 見出しコマンド群

use crate::{
  document::{HeadingLevel, HirNode, HirNodeKind},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{IndexPolicy, extract_inline_nodes},
      opt_args::{self, OptKey, collect_command_opt_args},
    },
    syntax::view::CommandView,
  },
};

/// 見出しの `[label=...]`（`\ref` からの参照用）
const LABEL: OptKey<String> = opt_args::string("label");

/// 見出しコマンドの共通処理
///
/// ラベルとタイトルだけを構造化し、採番は後段に委ねる。
///
/// # Errors
///
/// 引数不足・過剰、または `[label=...]` の値型不一致でエラーを返します。
pub(super) fn heading(
  view: &CommandView<'_>,
  ctx: &EvalContext<'_>,
  level: HeadingLevel,
) -> Result<Vec<HirNode>, EvalError> {
  let name = level.command_name();

  let opt_args = collect_command_opt_args(view, &[LABEL.decl()])?;
  let label = opt_args.get(LABEL);

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: expected_name(level).to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().into(),
    });
  }

  let id = ctx.alloc(view.span());
  // 見出しタイトルは目次・走り文へも展開されうる複製文脈なので `\index` を拒否する
  let title = extract_inline_nodes(view.source(), ctx, first_arg, IndexPolicy::Reject)?;

  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::Heading {
      level,
      title,
      label,
    },
  )]);
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
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_block_handler, test_support};

  #[test]
  fn heading_captures_label_and_is_numbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[label=sec:foo]{Title}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_block_handler(|ctx| return heading(&view, ctx, HeadingLevel::Section)).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Heading { level, label, .. } = &result[0].kind else {
      panic!("Heading が期待されます");
    };
    assert_eq!(*level, HeadingLevel::Section);
    // 見出しの numbered は HIR には存在しない（frontend が作る見出しは常に採番対象で
    // 構造的に一意に決まるため、HirNodeKind::Heading はそもそもフィールドを持たない）
    assert_eq!(label.as_deref(), Some("sec:foo"));
  }

  #[test]
  fn heading_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[draft=true]{Title}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = run_block_handler(|ctx| return heading(&view, ctx, HeadingLevel::Section));

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }
}
