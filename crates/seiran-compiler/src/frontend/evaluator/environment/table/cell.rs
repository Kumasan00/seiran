//! `\row` の `&` 区切り区画からセル（[`HirTableCell`]）を構築する

use crate::{
  document::{HirInline, HirInlineKind, HirTableCell},
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity,
      environment::table::grid::Coverage,
      inline::{IndexPolicy, evaluate_inline_children, evaluate_inline_elements},
      opt_args::{self, OptKey, collect_command_opt_args},
    },
    syntax::{CstElement, SyntaxKind, token::TokenKind, view::CommandView},
  },
  source::Span,
};

/// `\cell[span=N]`（横に連結する列数。1 以上の整数）
const SPAN: OptKey<u32> = opt_args::positive_int("span");
/// `\cell[rowspan=N]`（下に結合する行数。1 以上の整数）
const ROW_SPAN: OptKey<u32> = opt_args::positive_int("rowspan");
/// `\cell[rule_below]`（跨ぐ列の範囲の下に部分罫を引く）
const RULE_BELOW: OptKey<bool> = opt_args::boolean("rule_below");

/// `&` 分割後の 1 区画を [`HirTableCell`] に変換する
///
/// `column` は区画の開始列、`coverage` は上の行の結合が覆う列。覆われる列に中身のある区画・`\cell` を
/// 置くとエラーにする（空の区画は覆われる位置の書き方なので検査しない）。
pub(super) fn build_cell(
  source: &str,
  ctx: &EvalContext<'_>,
  elements: &[CstElement<'_>],
  empty_span: Span,
  index_policy: IndexPolicy,
  column: usize,
  coverage: &Coverage,
) -> Result<HirTableCell, EvalError> {
  let mut cell_view: Option<CommandView<'_>> = None;
  let mut has_other_content = false;
  for element in elements {
    match element {
      CstElement::Token(token) => match token.kind {
        TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment | TokenKind::LBrace | TokenKind::RBrace => {},
        _ => has_other_content = true,
      },
      CstElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let candidate = CommandView::new(node, source);
          if candidate.name() == "cell" {
            if cell_view.is_some() {
              // 同一区画に \cell が 2 つ — `&` の書き忘れ
              return Err(EvalError::TableCellMixedContent {
                span: node.span.into(),
              });
            }
            cell_view = Some(candidate);
          } else {
            has_other_content = true;
          }
        } else {
          has_other_content = true;
        }
      },
    }
  }

  if let Some(cell_cmd) = cell_view {
    if has_other_content {
      return Err(EvalError::TableCellMixedContent {
        span: cell_cmd.span().into(),
      });
    }
    let cell = evaluate_cell_command(&cell_cmd, ctx, index_policy)?;
    if let Some(owner) = coverage.owner(column..column + cell.column_span as usize) {
      return Err(EvalError::TableCoveredSlotNotEmpty {
        span: cell_cmd.span().into(),
        cell: ctx.span_of(owner).into(),
      });
    }
    return Ok(cell);
  }

  let segment = segment_span(elements, empty_span);
  let id = ctx.alloc(segment);
  let content = trim_cell_content(evaluate_inline_elements(source, ctx, elements, index_policy)?);
  if !content.is_empty()
    && let Some(owner) = coverage.owner(column..column + 1)
  {
    return Err(EvalError::TableCoveredSlotNotEmpty {
      span: segment.into(),
      cell: ctx.span_of(owner).into(),
    });
  }
  return Ok(HirTableCell {
    id,
    content,
    column_span: 1,
    row_span: 1,
    rule_below: false,
  });
}

/// `\cell[span=N, rowspan=N, rule_below]{...}` を属性付きセルに変換する
fn evaluate_cell_command(
  view: &CommandView<'_>,
  ctx: &EvalContext<'_>,
  index_policy: IndexPolicy,
) -> Result<HirTableCell, EvalError> {
  let opts = collect_command_opt_args(view, &[SPAN.decl(), ROW_SPAN.decl(), RULE_BELOW.decl()])?;
  let column_span = opts.get(SPAN).unwrap_or(1);
  let row_span = opts.get(ROW_SPAN).unwrap_or(1);
  let rule_below = opts.get(RULE_BELOW).unwrap_or(false);

  let arg = arity::exactly_one_arg(view, "セル内容")?;

  let id = ctx.alloc(view.span());
  let content = trim_cell_content(evaluate_inline_children(view.source(), ctx, arg, index_policy)?);
  return Ok(HirTableCell {
    id,
    content,
    column_span,
    row_span,
    rule_below,
  });
}

/// セル内容の前後の空白由来 `Text` ノードをトリムする
fn trim_cell_content(mut content: Vec<HirInline>) -> Vec<HirInline> {
  while matches!(content.first().map(|inline| return &inline.kind), Some(HirInlineKind::Text(t)) if t.trim().is_empty())
  {
    content.remove(0);
  }
  if let Some(HirInlineKind::Text(t)) = content.first_mut().map(|inline| return &mut inline.kind) {
    *t = t.trim_start().to_string();
  }
  while matches!(content.last().map(|inline| return &inline.kind), Some(HirInlineKind::Text(t)) if t.trim().is_empty())
  {
    content.pop();
  }
  if let Some(HirInlineKind::Text(t)) = content.last_mut().map(|inline| return &mut inline.kind) {
    *t = t.trim_end().to_string();
  }
  return content;
}

/// `&` 分割後の区画全体を覆うソース位置を返す
///
/// 区画が空（`a & & b` の中央など）なら、呼び出し元が渡した直前の区切り位置を使う。
fn segment_span(elements: &[CstElement<'_>], empty_span: Span) -> Span {
  let mut span: Option<Span> = None;
  for element in elements {
    let element_span = match element {
      CstElement::Token(token) => token.span,
      CstElement::Node(node) => node.span,
    };
    span = Some(span.map_or(element_span, |current| return current.merge(element_span)));
  }
  return span.unwrap_or(empty_span);
}

/// セル内容に強制改行（`\\`）が含まれるかを再帰的に判定する
pub(super) fn contains_line_break(nodes: &[HirInline]) -> bool {
  return nodes.iter().any(|node| match &node.kind {
    HirInlineKind::LineBreak => return true,
    HirInlineKind::Styled { children, .. } | HirInlineKind::Colored { children, .. } => {
      return contains_line_break(children);
    },
    // 見るのはセル本文そのものの改行だけ。`Link` の表示テキスト・`Footnote` の本体は
    // セル本文とは別のスコープなので、そこに `\\` があってもセルの改行としては数えない。
    HirInlineKind::Text(_)
    | HirInlineKind::Code(_)
    | HirInlineKind::Math(_)
    | HirInlineKind::Symbol(_)
    | HirInlineKind::NoIndent
    | HirInlineKind::Ref { .. }
    | HirInlineKind::Link { .. }
    | HirInlineKind::Cite { .. }
    | HirInlineKind::Footnote { .. }
    | HirInlineKind::Index { .. } => return false,
  });
}
