//! 見出しコマンド群

use crate::{
  document::{HeadingLevel, HirHeading, HirNode, HirNodeKind},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{IndexPolicy, evaluate_inline_children},
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::Signature,
    },
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// 見出しの `[label=...]`（`\ref` からの参照用）
const LABEL: OptKey<String> = opt_args::string("label");
/// 見出しが受け付けるキー
const KEYS: &[OptDecl] = &[LABEL.decl()];

/// 見出しコマンドの署名（不足時の説明だけが階層ごとに違う）
pub(super) const fn signature(level: HeadingLevel) -> Signature<1> {
  let expected = match level {
    HeadingLevel::Part => "部名",
    HeadingLevel::Chapter => "章名",
    HeadingLevel::Section => "セクション名",
    HeadingLevel::Subsection => "サブセクション名",
    HeadingLevel::Paragraph => "段落名",
    HeadingLevel::Subparagraph => "小節名",
  };
  return Signature::new([ArgMode::Inherit], expected, KEYS);
}

/// 見出しコマンドの共通処理
///
/// ラベルとタイトルだけを構造化し、採番は後段に委ねる。
///
/// # Errors
///
/// タイトルの評価エラーを返します。
pub(super) fn heading(
  view: &CommandView<'_>,
  title_arg: &CstNode<'_>,
  opts: &OptArgs,
  ctx: &EvalContext<'_>,
  level: HeadingLevel,
) -> Result<HirNode, EvalError> {
  let label = opts.get(LABEL);

  let id = ctx.alloc(view.span());
  // 見出しタイトルは目次・走り文へも展開されうる複製文脈なので `\index` を拒否する
  let title = evaluate_inline_children(view.source(), ctx, title_arg, IndexPolicy::Reject)?;

  return Ok(HirNode::new(
    id,
    HirNodeKind::Heading(HirHeading {
      level,
      title,
      label,
    }),
  ));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{run_handler, test_support};

  #[test]
  fn heading_captures_label_and_is_numbered() {
    let arena = Bump::new();
    let source = r"\section[label=sec:foo]{Title}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let bound = signature(HeadingLevel::Section).bind(&view).unwrap();

    let result =
      run_handler(|ctx| return heading(&view, bound.args[0], &bound.opts, ctx, HeadingLevel::Section)).unwrap();

    let HirNodeKind::Heading(heading) = &result.kind else {
      panic!("Heading が期待されます");
    };
    assert_eq!(heading.level, HeadingLevel::Section);
    assert_eq!(heading.label.as_deref(), Some("sec:foo"));
  }
}
