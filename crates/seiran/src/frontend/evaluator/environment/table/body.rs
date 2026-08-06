//! `table` 本体の走査（`\head` / `\row` / `\caption`）と列数の解決・検証

use super::cell::{build_cell, contains_line_break};
use crate::{
  frontend::{
    evaluator::{
      EvalError,
      environment::{body_scan, caption::extract_caption},
      opt_args::{OptType, OptValue, collect_command_opt_args},
    },
    span_ext::ToSourceSpan,
    syntax::{
      SyntaxKind,
      ast::{CommandView, EnvironmentView},
      green::GreenElement,
      token::TokenKind,
    },
  },
  model::{CaptionPosition, ColumnAlign, ColumnWidth, HirBuilder, HirInline, HirTableCell, HirTableRow},
  source::Span,
};

/// 本体走査で収集した行・キャプション情報
pub(super) struct TableBody {
  /// `\head` 行（列数確定後のセル数検証に使うソース位置つき）
  pub(super) head: Vec<(HirTableRow, miette::SourceSpan)>,
  /// `\row` 行（列数確定後のセル数検証に使うソース位置つき）
  pub(super) rows: Vec<(HirTableRow, miette::SourceSpan)>,
  /// `\caption` の内容（未指定なら `None`）
  pub(super) caption: Option<Vec<HirInline>>,
  /// キャプションを表の上下どちらに配置するか
  pub(super) caption_position: CaptionPosition,
}

/// 本体から `\head` / `\row` / `\caption` を走査して [`TableBody`] に収集する
pub(super) fn scan_table_body(view: &EnvironmentView, builder: &HirBuilder) -> Result<TableBody, EvalError> {
  let source = view.source();
  let mut head: Vec<(HirTableRow, miette::SourceSpan)> = Vec::new();
  let mut rows: Vec<(HirTableRow, miette::SourceSpan)> = Vec::new();
  let mut caption: Option<Vec<HirInline>> = None;
  // `\caption` が最初の行（`\head` / `\row`）よりソース上で先に現れた場合のみ Top
  let mut caption_position = CaptionPosition::Bottom;

  if let Some(body) = view.body() {
    for cmd_view in body_scan::strict_command_calls(
      source,
      body,
      "table",
      &["head", "row", "caption"],
      "\\head と \\row と \\caption",
    )? {
      match cmd_view.name() {
        "head" => {
          if !head.is_empty() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "table".to_string(),
              name: "head".to_string(),
              span: cmd_view.span().to_source_span(),
            });
          }
          head = extract_head(&cmd_view, builder)?;
        },
        "row" => {
          let span = cmd_view.span().to_source_span();
          rows.push((extract_row(&cmd_view, builder)?, span));
        },
        "caption" => {
          if caption.is_some() {
            return Err(EvalError::DuplicateCommandInEnvironment {
              env: "table".to_string(),
              name: "caption".to_string(),
              span: cmd_view.span().to_source_span(),
            });
          }
          if head.is_empty() && rows.is_empty() {
            caption_position = CaptionPosition::Top;
          }
          caption = Some(extract_caption(&cmd_view, builder)?);
        },
        _ => unreachable!("許可リスト外は strict_command_calls がエラーにする"),
      }
    }
  }

  return Ok(TableBody {
    head,
    rows,
    caption,
    caption_position,
  });
}

/// `\head{\row{...} ...}` からヘッダ行を抽出する
fn extract_head(view: &CommandView, builder: &HirBuilder) -> Result<Vec<(HirTableRow, miette::SourceSpan)>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "head".to_string(),
      expected: "\\row コマンド".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "head".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let source = view.source();
  let mut rows = Vec::new();
  for child in arg.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::ParagraphBreak
        | TokenKind::Comment
        | TokenKind::LBrace
        | TokenKind::RBrace => {},
        _ => {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: "table".to_string(),
            expected: "\\head の中の \\row".to_string(),
            span: token.span.to_source_span(),
          });
        },
      },
      GreenElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let row_view = CommandView::new(node, source);
          if row_view.name() == "row" {
            let span = row_view.span().to_source_span();
            rows.push((extract_row(&row_view, builder)?, span));
          } else {
            return Err(EvalError::UnexpectedCommandInEnvironment {
              env: "table".to_string(),
              name: row_view.name().to_string(),
              expected: "\\head の中の \\row".to_string(),
              span: node.span.to_source_span(),
            });
          }
        } else {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: "table".to_string(),
            expected: "\\head の中の \\row".to_string(),
            span: node.span.to_source_span(),
          });
        }
      },
    }
  }
  if rows.is_empty() {
    return Err(EvalError::MissingCommandArgument {
      name: "head".to_string(),
      expected: "\\row コマンド".to_string(),
      span: view.span().to_source_span(),
    });
  }
  return Ok(rows);
}

/// `\row[rule_above]{A & B & \cell[span=2]{C}}` から 1 行を抽出する
fn extract_row(view: &CommandView, builder: &HirBuilder) -> Result<HirTableRow, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("rule_above", OptType::Bool)])?;
  let rule_above = opt_args
    .iter()
    .any(|(key, value)| return key == "rule_above" && matches!(value, OptValue::Bool(true)));

  let Some(arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "row".to_string(),
      expected: "セル内容".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "row".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let source = view.source();
  let id = builder.alloc(view.span());
  let mut cells: Vec<HirTableCell> = Vec::new();
  let mut segment: Vec<GreenElement> = Vec::new();
  // 空セルには覆う要素がないので、直前の区切り位置を 0 幅の位置として使う
  let mut empty_cell_span = Span::new(arg.span.start, arg.span.start);
  for child in arg.children {
    if let GreenElement::Token(token) = child
      && token.kind == TokenKind::Ampersand
    {
      cells.push(build_cell(source, builder, &segment, empty_cell_span)?);
      segment.clear();
      empty_cell_span = Span::new(token.span.end, token.span.end);
    } else {
      segment.push(*child);
    }
  }
  cells.push(build_cell(source, builder, &segment, empty_cell_span)?);

  for cell in &cells {
    if contains_line_break(&cell.content) {
      return Err(EvalError::LineBreakInTableCell {
        span: view.span().to_source_span(),
      });
    }
  }

  return Ok(HirTableRow {
    id,
    cells,
    rule_above,
  });
}

/// 列数を決定し、全行のセル数（`span` 合計）が一致するか検証する
pub(super) fn resolve_column_count(
  columns_tokens: Option<&[ColumnAlign]>,
  widths_tokens: Option<&[ColumnWidth]>,
  head: &[(HirTableRow, miette::SourceSpan)],
  rows: &[(HirTableRow, miette::SourceSpan)],
  view: &EnvironmentView,
) -> Result<usize, EvalError> {
  let column_count = match (columns_tokens, widths_tokens) {
    (Some(c), Some(w)) => {
      if c.len() != w.len() {
        return Err(EvalError::TableColumnsWidthsMismatch {
          columns: c.len(),
          widths: w.len(),
          span: view.span().to_source_span(),
        });
      }
      c.len()
    },
    (Some(c), None) => c.len(),
    (None, Some(w)) => w.len(),
    (None, None) => head.iter().chain(rows.iter()).map(|(row, _)| return row_span_sum(row)).max().unwrap_or(0),
  };

  for (row, span) in head.iter().chain(rows.iter()) {
    let actual = row_span_sum(row);
    if actual != column_count {
      return Err(EvalError::TableRowCellCountMismatch {
        expected: column_count,
        actual,
        span: *span,
      });
    }
  }

  return Ok(column_count);
}

/// 行のセル数（`span` 合計）を返す
fn row_span_sum(row: &HirTableRow) -> usize { return row.cells.iter().map(|cell| return cell.span as usize).sum(); }
