//! `table` 本体の走査（`\head` / `\row` / `\caption`）と列数の解決・検証

use crate::{
  document::{CaptionPosition, ColumnAlign, ColumnWidth, HirInline, HirTableCell, HirTableRow, RowSeparator},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      environment::{
        body_scan, caption,
        table::{
          cell::{build_cell, contains_line_break},
          grid::{self, Coverage},
        },
      },
      inline::InlinePolicy,
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::{Bound, Signature},
    },
    syntax::{
      ArgMode, CstElement, CstNode,
      token::TokenKind,
      view::{CommandView, EnvironmentView},
    },
  },
  source::Span,
};

/// `\row[separator=rule|gap]`（行の上の境界に置く区切り）
const SEPARATOR: OptKey<String> = opt_args::string("separator");
/// `\row` が受け付けるキー
const ROW_KEYS: &[OptDecl] = &[SEPARATOR.decl()];

/// `\head` の署名
const HEAD: Signature<1> = Signature::new([ArgMode::Inherit], "\\row コマンド", &[]);
/// `\row` の署名（本体とヘッダの中で共有する）
const ROW: Signature<1> = Signature::new([ArgMode::Inherit], "セル内容", ROW_KEYS);

/// `table` 環境の本体に書けるコマンド
#[derive(Debug, Clone, Copy)]
enum TableCommand {
  /// `\head{\row{...}}` — ヘッダ行
  Head,
  /// `\row[...]{...}` — 本体行
  Row,
  /// `\caption{...}` — 表のキャプション
  Caption,
}

/// `table` 環境の本体で許可するコマンドと種別と署名
const TABLE_COMMANDS: &[(&str, TableCommand, Signature<1>)] = &[
  ("head", TableCommand::Head, HEAD),
  ("row", TableCommand::Row, ROW),
  ("caption", TableCommand::Caption, caption::SIGNATURE),
];

/// 本体走査で収集した行・キャプション情報
pub(super) struct TableBody {
  /// `\head` 行
  pub(super) head: Vec<HirTableRow>,
  /// `\row` 行
  pub(super) rows: Vec<HirTableRow>,
  /// `\caption` の内容（未指定なら `None`）
  pub(super) caption: Option<Vec<HirInline>>,
  /// キャプションを表の上下どちらに配置するか
  pub(super) caption_position: CaptionPosition,
}

/// 本体から `\head` / `\row` / `\caption` を走査して [`TableBody`] に収集する
pub(super) fn scan_table_body(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<TableBody, EvalError> {
  let source = view.source();
  let mut head: Vec<HirTableRow> = Vec::new();
  let mut rows: Vec<HirTableRow> = Vec::new();
  let mut caption: Option<Vec<HirInline>> = None;
  let mut coverage = Coverage::default();
  // `\caption` が最初の行（`\head` / `\row`）よりソース上で先に現れた場合のみ Top
  let mut caption_position = CaptionPosition::Bottom;

  if let Some(body) = view.body() {
    for (command, cmd_view, Bound { args: [arg], opts }) in body_scan::collect_allowed_commands(
      source,
      body.children,
      "table",
      TABLE_COMMANDS,
      "\\head と \\row と \\caption",
    )? {
      match command {
        TableCommand::Head => {
          if !head.is_empty() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "table".to_string(),
              name: "head".to_string(),
              span: cmd_view.span().into(),
            });
          }
          head = evaluate_head(&cmd_view, arg, ctx)?;
        },
        TableCommand::Row => {
          rows.push(evaluate_row(&cmd_view, arg, &opts, ctx, InlinePolicy::Allow, &mut coverage)?);
        },
        TableCommand::Caption => {
          if caption.is_some() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "table".to_string(),
              name: "caption".to_string(),
              span: cmd_view.span().into(),
            });
          }
          if head.is_empty() && rows.is_empty() {
            caption_position = CaptionPosition::Top;
          }
          caption = Some(caption::evaluate_caption(&cmd_view, arg, ctx)?);
        },
      }
    }
  }

  // 表の最初の行（`head[0]`、ヘッダが無ければ `rows[0]`）の上と、ヘッダ直後（ヘッダがあるときの `rows[0]`）の上は
  // style の枠罫の持ち場。`rows[0]` はどちらかに必ず当たり、`\head` がソース上で後にあっても本体の先頭行のまま
  for row in head.first().into_iter().chain(rows.first()) {
    if row.separator.is_some() {
      return Err(EvalError::TableSeparatorOnFrameBoundary {
        span: ctx.span_of(row.id).into(),
      });
    }
  }

  // 行結合は行群ごとに収まり、`rule_below` は行群の境界と `separator` を避ける
  for group in [&head, &rows] {
    grid::check_row_group(group, ctx)?;
  }

  return Ok(TableBody {
    head,
    rows,
    caption,
    caption_position,
  });
}

/// `\head{\row{...} ...}` をヘッダ行に変換する
///
/// ヘッダ行は表が改ページするたび全ページへ再描画される複製文脈なので、出現ページが一意に
/// 定まらない。セル内の `\index` は [`InlinePolicy::RejectIndex`] で拒否する。
fn evaluate_head(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  ctx: &EvalContext<'_>,
) -> Result<Vec<HirTableRow>, EvalError> {
  let source = view.source();
  let mut rows = Vec::new();
  let mut coverage = Coverage::default();
  for (
    (),
    row_view,
    Bound {
      args: [row_arg],
      opts,
    },
  ) in
    body_scan::collect_allowed_commands(source, arg.children, "table", &[("row", (), ROW)], "\\head の中の \\row")?
  {
    rows.push(evaluate_row(&row_view, row_arg, &opts, ctx, InlinePolicy::RejectIndex, &mut coverage)?);
  }
  if rows.is_empty() {
    return Err(EvalError::MissingCommandArgument {
      name: "head".to_string(),
      expected: "\\row コマンド".to_string(),
      span: view.span().into(),
    });
  }
  return Ok(rows);
}

/// `\row[separator=rule]{A & B & \cell[span=2]{C}}` を 1 行に変換する
fn evaluate_row(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  opts: &OptArgs,
  ctx: &EvalContext<'_>,
  inline_policy: InlinePolicy,
  coverage: &mut Coverage,
) -> Result<HirTableRow, EvalError> {
  let separator = match opts.get(SEPARATOR) {
    Some(value) => {
      let Ok(separator) = value.parse::<RowSeparator>() else {
        return Err(EvalError::InvalidOptArgValue {
          name: "row".to_string(),
          key: "separator".to_string(),
          expected: "rule / gap".to_string(),
          span: view.span().into(),
        });
      };
      Some(separator)
    },
    None => None,
  };

  let source = view.source();
  let id = ctx.alloc(view.span());
  let mut cells: Vec<HirTableCell> = Vec::new();
  let mut segment: Vec<CstElement<'_>> = Vec::new();
  // 空セルには覆う要素がないので、直前の区切り位置を 0 幅の位置として使う
  let mut empty_cell_span = Span::new(arg.span.start, arg.span.start);
  let mut column = 0usize;
  for child in arg.children {
    if let CstElement::Token(token) = child
      && token.kind == TokenKind::Ampersand
    {
      let cell = build_cell(source, ctx, &segment, empty_cell_span, inline_policy, column, coverage)?;
      column += cell.column_span as usize;
      cells.push(cell);
      segment.clear();
      empty_cell_span = Span::new(token.span.end, token.span.end);
    } else {
      segment.push(*child);
    }
  }
  cells.push(build_cell(source, ctx, &segment, empty_cell_span, inline_policy, column, coverage)?);
  coverage.advance(&cells);

  for cell in &cells {
    if contains_line_break(&cell.content) {
      return Err(EvalError::LineBreakInTableCell {
        span: view.span().into(),
      });
    }
  }

  return Ok(HirTableRow {
    id,
    cells,
    separator,
  });
}

/// 列数を決定し、全行のセル数（`column_span` 合計）が一致するか検証する
pub(super) fn resolve_column_count(
  columns_tokens: Option<&[ColumnAlign]>,
  widths_tokens: Option<&[ColumnWidth]>,
  head: &[HirTableRow],
  rows: &[HirTableRow],
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
) -> Result<usize, EvalError> {
  let column_count = match (columns_tokens, widths_tokens) {
    (Some(c), Some(w)) => {
      if c.len() != w.len() {
        return Err(EvalError::TableColumnsWidthsMismatch {
          columns: c.len(),
          widths: w.len(),
          span: view.span().into(),
        });
      }
      c.len()
    },
    (Some(c), None) => c.len(),
    (None, Some(w)) => w.len(),
    (None, None) => head.iter().chain(rows.iter()).map(row_column_span_sum).max().unwrap_or(0),
  };

  for row in head.iter().chain(rows.iter()) {
    let actual = row_column_span_sum(row);
    if actual != column_count {
      return Err(EvalError::TableRowCellCountMismatch {
        expected: column_count,
        actual,
        span: ctx.span_of(row.id).into(),
      });
    }
  }

  return Ok(column_count);
}

/// 行のセル数（`column_span` 合計）を返す
fn row_column_span_sum(row: &HirTableRow) -> usize {
  return row.cells.iter().map(|cell| return cell.column_span as usize).sum();
}
