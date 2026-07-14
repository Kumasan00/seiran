//! `table` 本体の走査（`\head` / `\row` / `\caption`）と列数の解決・検証

use model::{CaptionPosition, ColumnAlign, ColumnWidth, InlineNode, TableCell, TableRow};

use super::cell::{build_cell, contains_line_break};
use crate::{
  evaluator::{
    EvalError,
    environment::{body_scan, caption::extract_caption},
    opt_args::{OptType, OptValue, collect_command_opt_args},
  },
  syntax::{
    SyntaxKind,
    ast::{CommandView, EnvironmentView},
    green::GreenElement,
    token::TokenKind,
  },
};

/// 本体走査で収集した行・キャプション情報
pub(super) struct TableBody {
  pub(super) head: Vec<(TableRow, miette::SourceSpan)>,
  pub(super) rows: Vec<(TableRow, miette::SourceSpan)>,
  pub(super) caption: Option<Vec<InlineNode>>,
  pub(super) caption_position: CaptionPosition,
}

/// 本体から `\head` / `\row` / `\caption` を走査して [`TableBody`] に収集する
///
/// `\head` / `\caption` は高々 1 回（重複は [`EvalError::DuplicateCommandInEnvironment`]）。
/// 行はソース位置とともに収集し、列数確定後のセル数検証に使う。キャプション位置は、
/// `\caption` が最初の行（`\head` / `\row`）よりソース上で先に現れた場合のみ `Top`。
pub(super) fn scan_table_body(view: &EnvironmentView) -> Result<TableBody, EvalError> {
  let source = view.source();
  // 行はソース位置とともに収集し、列数確定後にセル数を検証する
  let mut head: Vec<(TableRow, miette::SourceSpan)> = Vec::new();
  let mut rows: Vec<(TableRow, miette::SourceSpan)> = Vec::new();
  let mut caption: Option<Vec<InlineNode>> = None;
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
              span: cmd_view.span().into(),
            });
          }
          head = extract_head(&cmd_view)?;
        },
        "row" => {
          let span = cmd_view.span().into();
          rows.push((extract_row(&cmd_view)?, span));
        },
        "caption" => {
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
          caption = Some(extract_caption(&cmd_view)?);
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
///
/// 引数の直下に書けるのは `\row` のみ。トリビア（空白・改行・段落区切り・コメント・
/// 引数の括弧）はスキップし、それ以外はエラーにする。
fn extract_head(view: &CommandView) -> Result<Vec<(TableRow, miette::SourceSpan)>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "head".to_string(),
      expected: "\\row コマンド".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "head".to_string(),
      span: view.span().into(),
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
            span: token.span.into(),
          });
        },
      },
      GreenElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let row_view = CommandView::new(node, source);
          if row_view.name() == "row" {
            let span = row_view.span().into();
            rows.push((extract_row(&row_view)?, span));
          } else {
            return Err(EvalError::UnexpectedCommandInEnvironment {
              env: "table".to_string(),
              name: row_view.name().to_string(),
              expected: "\\head の中の \\row".to_string(),
              span: node.span.into(),
            });
          }
        } else {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: "table".to_string(),
            expected: "\\head の中の \\row".to_string(),
            span: node.span.into(),
          });
        }
      },
    }
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

/// `\row[rule_above]{A & B & \cell[span=2]{C}}` から 1 行を抽出する
///
/// 引数の子要素をトップレベルの `&`（[`TokenKind::Ampersand`]）で分割し、
/// 各区画をセルに変換する。区画全体が `\cell` コマンドの場合は属性付きセルとして、
/// それ以外はインライン内容のセルとして評価する。
fn extract_row(view: &CommandView) -> Result<TableRow, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("rule_above", OptType::Bool)])?;
  let rule_above = opt_args.iter().any(|(key, value)| key == "rule_above" && matches!(value, OptValue::Bool(true)));

  let Some(arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "row".to_string(),
      expected: "セル内容".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "row".to_string(),
      span: view.span().into(),
    });
  }

  let source = view.source();
  let mut cells: Vec<TableCell> = Vec::new();
  let mut segment: Vec<GreenElement> = Vec::new();
  for child in arg.children {
    if let GreenElement::Token(token) = child
      && token.kind == TokenKind::Ampersand
    {
      cells.push(build_cell(source, &segment)?);
      segment.clear();
    } else {
      segment.push(*child);
    }
  }
  cells.push(build_cell(source, &segment)?);

  for cell in &cells {
    if contains_line_break(&cell.content) {
      return Err(EvalError::LineBreakInTableCell {
        span: view.span().into(),
      });
    }
  }

  return Ok(TableRow { cells, rule_above });
}

/// 列数を決定し、全行のセル数（`span` 合計）が一致するか検証する
///
/// 列数は `columns` / `widths` の明示指定を優先し、両方未指定なら行のセル数（`span` 合計）の
/// 最大値を採る。`columns` と `widths` の長さが食い違う場合は
/// [`EvalError::TableColumnsWidthsMismatch`]、列数と一致しない行があれば
/// [`EvalError::TableRowCellCountMismatch`] を返す。
pub(super) fn resolve_column_count(
  columns_tokens: Option<&[ColumnAlign]>,
  widths_tokens: Option<&[ColumnWidth]>,
  head: &[(TableRow, miette::SourceSpan)],
  rows: &[(TableRow, miette::SourceSpan)],
  view: &EnvironmentView,
) -> Result<usize, EvalError> {
  // 列数の決定: columns / widths の明示指定を優先し、両方未指定なら行のセル数（span 合計）の最大値
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
    (None, None) => head.iter().chain(rows.iter()).map(|(row, _)| row_span_sum(row)).max().unwrap_or(0),
  };

  // 各行のセル数（span 合計）が列数と一致するか検証する
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
fn row_span_sum(row: &TableRow) -> usize { return row.cells.iter().map(|cell| cell.span as usize).sum(); }
