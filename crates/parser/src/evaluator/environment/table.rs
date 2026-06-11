//! 表環境 — `table`
//!
//! `\begin{table}...\end{table}` を [`DocNode::Table`] に変換します。
//! 本体に書けるのは `\head`（任意・1 回）、`\row`（複数可）、`\caption`（任意・1 回）のみで、
//! それ以外のコマンド・テキストはエラーとして報告します。
//!
//! ## 任意引数
//!
//! - `[columns="left center right"]` — 列ごとの揃え（空白区切り、フルスペルのみ）
//! - `[widths="auto auto 5cm"]` — 列ごとの幅（`auto` / `<num>mm|cm` / `0.3` 比率 / `*` 残り幅）
//! - `[label=tab:foo]` — `\ref` 解決用ラベル
//! - `[breakable=false]` — 改ページによる分割の禁止（既定は分割可）
//!
//! ## 本体内のコマンド
//!
//! - `\head{\row{...} ...}` — ヘッダ行（改ページ時に再描画される）
//! - `\row[rule_above]{A & B & C}` — 本体行。セル区切りは `&`、
//!   特殊属性が必要なセルだけ `\cell[span=N]{...}` にエスカレートする
//! - `\caption{...}` — キャプション（出現位置が最初の行より前なら上、後なら下に配置）

use read_style::CounterName;
use syntax::{
  SyntaxKind,
  ast::{CommandView, EnvironmentView},
  green::GreenElement,
  token::TokenKind,
};
use types::{ColumnAlign, ColumnWidth, Length};

use crate::{
  document::{CaptionPosition, DocNode, InlineNode, TableCell, TableRow},
  evaluator::{
    EvalError, Evaluator,
    environment::{body_scan, figure::extract_caption},
    inline::{extract_inline_nodes, extract_inline_nodes_from_elements},
    opt_args::{OptType, OptValue, collect_command_opt_args, collect_environment_opt_args},
  },
};

/// `table` 環境を評価する
///
/// [`crate::evaluator::counter::CounterRegistry::increment`] で `CounterName::Table` の
/// 通し番号を発番し、本体内の `\head` / `\row` / `\caption` を抽出して
/// [`DocNode::Table`] を生成する。`columns` / `widths` は列数に正規化され、
/// 各行のセル数（`span` 合計）が列数と一致しない場合はエラーになる。
///
/// # Errors
///
/// 未知の任意引数キー、揃え / 幅トークンの不正、セル数の不一致、
/// `\row` の欠如などが発生した場合にエラーを返します。
pub(super) fn table(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(
    view,
    &[
      ("columns", OptType::String),
      ("widths", OptType::String),
      ("label", OptType::String),
      ("breakable", OptType::Bool),
    ],
  )?;

  let mut columns_spec: Option<String> = None;
  let mut widths_spec: Option<String> = None;
  let mut label: Option<String> = None;
  let mut breakable = true;
  for (key, value) in opt_args {
    match (key.as_str(), value) {
      ("columns", OptValue::String(s)) => columns_spec = Some(s),
      ("widths", OptValue::String(s)) => widths_spec = Some(s),
      ("label", OptValue::String(s)) => label = Some(s),
      ("breakable", OptValue::Bool(b)) => breakable = b,
      _ => {},
    }
  }

  let number = evaluator.registry.increment(CounterName::Table);
  if let Some(l) = &label
    && !evaluator.registry.register_label(l.clone(), CounterName::Table, &number)
  {
    return Err(EvalError::DuplicateLabel {
      label: l.clone(),
      span: view.span().into(),
    });
  }

  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "table".to_string(),
      span: view.span().into(),
    });
  }

  let columns_tokens = columns_spec.as_deref().map(|s| parse_columns_spec(s, view)).transpose()?;
  let widths_tokens = widths_spec.as_deref().map(|s| parse_widths_spec(s, view)).transpose()?;

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

  if head.is_empty() && rows.is_empty() {
    return Err(EvalError::MissingEnvironmentArgument {
      name: "table".to_string(),
      expected: "\\row コマンド".to_string(),
      span: view.span().into(),
    });
  }

  // 列数の決定: columns / widths の明示指定を優先し、両方未指定なら行のセル数（span 合計）の最大値
  let column_count = match (&columns_tokens, &widths_tokens) {
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

  let columns = columns_tokens.unwrap_or_else(|| vec![ColumnAlign::Left; column_count]);
  let widths = widths_tokens.unwrap_or_else(|| vec![ColumnWidth::Auto; column_count]);

  return Ok(vec![DocNode::Table {
    columns,
    widths,
    head: head.into_iter().map(|(row, _)| row).collect(),
    rows: rows.into_iter().map(|(row, _)| row).collect(),
    caption,
    caption_position,
    label,
    number,
    breakable,
  }]);
}

/// 行のセル数（`span` 合計）を返す
fn row_span_sum(row: &TableRow) -> usize { return row.cells.iter().map(|cell| cell.span as usize).sum(); }

/// `columns="left center right"` の値を [`ColumnAlign`] の列に変換する
fn parse_columns_spec(spec: &str, view: &EnvironmentView) -> Result<Vec<ColumnAlign>, EvalError> {
  let invalid = || EvalError::InvalidOptArgValue {
    name: "table".to_string(),
    key: "columns".to_string(),
    expected: "left / center / right の空白区切り".to_string(),
    span: view.span().into(),
  };
  let tokens: Vec<&str> = spec.split_whitespace().collect();
  if tokens.is_empty() {
    return Err(invalid());
  }
  return tokens.iter().map(|t| ColumnAlign::from_keyword(t).ok_or_else(invalid)).collect();
}

/// `widths="auto auto 5cm 0.3 *"` の値を [`ColumnWidth`] の列に変換する
fn parse_widths_spec(spec: &str, view: &EnvironmentView) -> Result<Vec<ColumnWidth>, EvalError> {
  let invalid = || EvalError::InvalidOptArgValue {
    name: "table".to_string(),
    key: "widths".to_string(),
    expected: "auto / <num>mm / <num>cm / 0〜1 の比率 / * の空白区切り".to_string(),
    span: view.span().into(),
  };
  let tokens: Vec<&str> = spec.split_whitespace().collect();
  if tokens.is_empty() {
    return Err(invalid());
  }
  return tokens.iter().map(|t| parse_width_token(t).ok_or_else(invalid)).collect();
}

/// `widths=` の 1 トークンを [`ColumnWidth`] に変換する
///
/// 受理する形式: `auto` / `*` / `<num>mm` / `<num>cm`（サフィックスは大小無視）/
/// `0` より大きく `1` 以下の小数（本文幅に対する比率）。
fn parse_width_token(token: &str) -> Option<ColumnWidth> {
  if token == "auto" {
    return Some(ColumnWidth::Auto);
  }
  if token == "*" {
    return Some(ColumnWidth::Flex);
  }
  let lower = token.to_ascii_lowercase();
  if let Some(stripped) = lower.strip_suffix("mm") {
    let value: f32 = stripped.parse().ok()?;
    if !(value.is_finite() && value > 0.0) {
      return None;
    }
    return Some(ColumnWidth::Fixed(Length::mm(value)));
  }
  if let Some(stripped) = lower.strip_suffix("cm") {
    let value: f32 = stripped.parse().ok()?;
    if !(value.is_finite() && value > 0.0) {
      return None;
    }
    return Some(ColumnWidth::Fixed(Length::cm(value)));
  }
  let ratio: f32 = lower.parse().ok()?;
  if !(ratio.is_finite() && ratio > 0.0 && ratio <= 1.0) {
    return None;
  }
  return Some(ColumnWidth::Ratio(ratio));
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

/// `&` 分割後の 1 区画を [`TableCell`] に変換する
///
/// 区画の非トリビア要素が `\cell` コマンド 1 つだけなら属性付きセルとして評価し、
/// `\cell` と他の内容が混在していればエラーにする。それ以外の区画は
/// インライン内容として評価し、前後の空白をトリムする。
fn build_cell(source: &str, elements: &[GreenElement]) -> Result<TableCell, EvalError> {
  let mut cell_view: Option<CommandView> = None;
  let mut has_other_content = false;
  for element in elements {
    match element {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment | TokenKind::LBrace | TokenKind::RBrace => {},
        _ => has_other_content = true,
      },
      GreenElement::Node(node) => {
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
    return extract_cell_command(&cell_cmd);
  }

  let content = extract_inline_nodes_from_elements(source, elements)?;
  return Ok(TableCell::new(trim_cell_content(content)));
}

/// `\cell[span=N]{...}` を属性付きセルに変換する
fn extract_cell_command(view: &CommandView) -> Result<TableCell, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("span", OptType::Number)])?;
  let mut span: u32 = 1;
  for (key, value) in opt_args {
    if let ("span", OptValue::Number(n)) = (key.as_str(), value) {
      // span は 1 以上の整数のみ受理する
      if !(n.is_finite() && n >= 1.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX)) {
        return Err(EvalError::InvalidOptArgValue {
          name: "cell".to_string(),
          key: "span".to_string(),
          expected: "1 以上の整数".to_string(),
          span: view.span().into(),
        });
      }
      #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
      {
        span = n as u32;
      }
    }
  }

  let Some(arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "cell".to_string(),
      expected: "セル内容".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "cell".to_string(),
      span: view.span().into(),
    });
  }

  let content = trim_cell_content(extract_inline_nodes(view.source(), arg)?);
  return Ok(TableCell { content, span });
}

/// セル内容の前後の空白由来 `Text` ノードをトリムする
///
/// `\row{Alice & 92}` の区切り前後の空白がセル内容に残ると揃えがずれるため、
/// 先頭・末尾の空白のみの `Text` を除去し、境界の `Text` の端も削る。
fn trim_cell_content(mut content: Vec<InlineNode>) -> Vec<InlineNode> {
  while matches!(content.first(), Some(InlineNode::Text(t)) if t.trim().is_empty()) {
    content.remove(0);
  }
  if let Some(InlineNode::Text(t)) = content.first_mut() {
    *t = t.trim_start().to_string();
  }
  while matches!(content.last(), Some(InlineNode::Text(t)) if t.trim().is_empty()) {
    content.pop();
  }
  if let Some(InlineNode::Text(t)) = content.last_mut() {
    *t = t.trim_end().to_string();
  }
  return content;
}

/// セル内容に強制改行（`\\`）が含まれるかを再帰的に判定する
fn contains_line_break(nodes: &[InlineNode]) -> bool {
  return nodes.iter().any(|node| match node {
    InlineNode::LineBreak => true,
    InlineNode::Emphasis(children)
    | InlineNode::Strong(children)
    | InlineNode::Code(children)
    | InlineNode::SansSerif(children) => contains_line_break(children),
    _ => false,
  });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{document::inline_nodes_to_plain_text, evaluator::lookup_env_parse_mode};

  /// テスト用 `parse` ラッパ
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// ソースを評価して最初の `DocNode::Table` を取り出すヘルパ
  fn eval_table(source: &str) -> Result<Vec<DocNode>, EvalError> {
    let arena = Bump::new();
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();
    return evaluator.evaluate_children(source, cst);
  }

  /// セル内容のプレーンテキストを行ごとに並べるヘルパ
  fn row_texts(rows: &[TableRow]) -> Vec<Vec<String>> {
    return rows
      .iter()
      .map(|row| row.cells.iter().map(|cell| inline_nodes_to_plain_text(&cell.content)).collect())
      .collect();
  }

  #[test]
  fn table_extracts_head_rows_and_caption() {
    // Arrange — 設計メモの代表例
    let source = r#"\begin{table}[columns="left center right", widths="auto auto 5cm"]
\head{
  \row{Name & Score & Rank}
}
\row{Alice & 92 & 1}
\row{Bob & 88 & 2}
\caption{得点表}
\end{table}"#;

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let DocNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      label,
      number,
      breakable,
    } = &result[0]
    else {
      panic!("Table が期待されます: {:?}", result[0]);
    };
    assert_eq!(columns, &[ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right]);
    assert_eq!(widths.len(), 3);
    assert!(matches!(widths[0], ColumnWidth::Auto));
    assert!(matches!(widths[1], ColumnWidth::Auto));
    assert!(matches!(widths[2], ColumnWidth::Fixed(l) if (l.to_mm() - 50.0).abs() < 1e-3));
    assert_eq!(row_texts(head), vec![vec!["Name", "Score", "Rank"]]);
    assert_eq!(row_texts(rows), vec![vec!["Alice", "92", "1"], vec!["Bob", "88", "2"]]);
    let caption = caption.as_ref().expect("caption あり");
    assert_eq!(inline_nodes_to_plain_text(caption), "得点表");
    // \caption は行より後 → Bottom
    assert_eq!(*caption_position, CaptionPosition::Bottom);
    assert!(label.is_none());
    assert!(!number.is_empty());
    assert!(*breakable);
  }

  #[test]
  fn table_caption_before_rows_yields_top_position() {
    // Arrange — \caption が最初の \row より前
    let source = r"\begin{table}\caption{c}\row{A & B}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table {
      caption_position, ..
    } = &result[0]
    else {
      panic!("Table が期待されます");
    };
    assert_eq!(*caption_position, CaptionPosition::Top);
  }

  #[test]
  fn table_infers_column_count_from_rows() {
    // Arrange — columns / widths 未指定なら行のセル数から列数を推定し、既定値で埋める
    let source = r"\begin{table}\row{A & B & C}\row{D & E & F}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table {
      columns, widths, ..
    } = &result[0]
    else {
      panic!("Table が期待されます");
    };
    assert_eq!(columns, &[ColumnAlign::Left; 3]);
    assert_eq!(widths, &[ColumnWidth::Auto; 3]);
  }

  #[test]
  fn table_cell_span_counts_toward_column_count() {
    // Arrange — \cell[span=2] は 2 列分として数えられる
    let source = r"\begin{table}\row{A & B & C}\row[rule_above]{\cell[span=2]{合計} & 180}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table { rows, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].rule_above);
    assert!(rows[1].rule_above);
    assert_eq!(rows[1].cells.len(), 2);
    assert_eq!(rows[1].cells[0].span, 2);
    assert_eq!(inline_nodes_to_plain_text(&rows[1].cells[0].content), "合計");
    assert_eq!(rows[1].cells[1].span, 1);
    assert_eq!(inline_nodes_to_plain_text(&rows[1].cells[1].content), "180");
  }

  #[test]
  fn table_rejects_row_cell_count_mismatch() {
    // Arrange — 2 列指定に 3 セルの行
    let source = r#"\begin{table}[columns="left right"]\row{A & B & C}\end{table}"#;

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(
      result,
      Err(EvalError::TableRowCellCountMismatch {
        expected: 2,
        actual: 3,
        ..
      })
    ));
  }

  #[test]
  fn table_rejects_columns_widths_length_mismatch() {
    // Arrange
    let source = r#"\begin{table}[columns="left right", widths="auto"]\row{A & B}\end{table}"#;

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(
      result,
      Err(EvalError::TableColumnsWidthsMismatch {
        columns: 2,
        widths: 1,
        ..
      })
    ));
  }

  #[test]
  fn table_rejects_unknown_align_keyword() {
    // Arrange — `l` 略記は不可
    let source = r#"\begin{table}[columns="l r"]\row{A & B}\end{table}"#;

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "columns"));
  }

  #[test]
  fn table_parses_width_ratio_and_flex() {
    // Arrange — 比率と * の混在
    let source = r#"\begin{table}[widths="0.3 * auto"]\row{A & B & C}\end{table}"#;

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table { widths, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert!(matches!(widths[0], ColumnWidth::Ratio(r) if (r - 0.3).abs() < 1e-6));
    assert!(matches!(widths[1], ColumnWidth::Flex));
    assert!(matches!(widths[2], ColumnWidth::Auto));
  }

  #[test]
  fn table_rejects_invalid_width_token() {
    // Arrange — 比率は 1 以下のみ
    let source = r#"\begin{table}[widths="1.5 auto"]\row{A & B}\end{table}"#;

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "widths"));
  }

  #[test]
  fn table_rejects_missing_rows() {
    // Arrange — 行が 1 つもない
    let source = r"\begin{table}\caption{c}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::MissingEnvironmentArgument { ref name, .. }) if name == "table"));
  }

  #[test]
  fn table_rejects_mixed_cell_and_text_in_segment() {
    // Arrange — \cell と通常テキストの混在
    let source = r"\begin{table}\row{\cell[span=2]{合計} extra & 180}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::TableCellMixedContent { .. })));
  }

  #[test]
  fn table_rejects_rowspan_key() {
    // Arrange — rowspan は未対応（許可キー外として報告される）
    let source = r"\begin{table}\row{\cell[rowspan=2]{X} & Y}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "rowspan"));
  }

  #[test]
  fn table_rejects_line_break_in_cell() {
    // Arrange — セル内の \\ は未対応
    let source = r"\begin{table}\row{A \\ B & C}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::LineBreakInTableCell { .. })));
  }

  #[test]
  fn table_rejects_duplicate_head() {
    // Arrange
    let source = r"\begin{table}\head{\row{A}}\head{\row{B}}\row{C}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::DuplicateCommandInEnvironment { ref name, .. }) if name == "head"));
  }

  #[test]
  fn table_rejects_stray_text_in_head() {
    // Arrange — \head 直下のテキストはエラー
    let source = r"\begin{table}\head{stray \row{A}}\row{B}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::UnexpectedContentInEnvironment { ref env, .. }) if env == "table"));
  }

  #[test]
  fn table_captures_label_and_sequential_numbers() {
    // Arrange — label 登録と通し番号
    let source = r"\begin{table}[label=tab:a]\row{A}\end{table}\begin{table}\row{B}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let DocNode::Table { label, number, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert_eq!(label.as_deref(), Some("tab:a"));
    // 既定書式 "{chapter}.{n}" — chapter 未進行なので "{空}.1" 相当の ".1" になるが、
    // ここでは連番部分のみを検証する
    assert!(number.ends_with('1'), "1 番目の表: {number}");
    let DocNode::Table { number, .. } = &result[1] else {
      panic!("Table が期待されます");
    };
    assert!(number.ends_with('2'), "2 番目の表: {number}");
  }

  #[test]
  fn table_breakable_false_is_captured() {
    // Arrange
    let source = r"\begin{table}[breakable=false]\row{A & B}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table { breakable, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert!(!breakable);
  }

  #[test]
  fn table_cell_content_is_trimmed() {
    // Arrange — `&` 前後の空白はセル内容に残らない
    let source = r"\begin{table}\row{  Alice   &   92  }\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table { rows, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert_eq!(row_texts(rows), vec![vec!["Alice", "92"]]);
  }

  #[test]
  fn table_cell_preserves_inline_styles() {
    // Arrange — セル内のインライン装飾は維持される
    let source = r"\begin{table}\row{\textbf{強調} & $x^2$}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table { rows, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert!(matches!(&rows[0].cells[0].content[0], InlineNode::Strong(_)));
    assert!(matches!(&rows[0].cells[1].content[0], InlineNode::InlineMath(_)));
  }

  #[test]
  fn table_empty_cell_is_allowed() {
    // Arrange — 空セル（連続する `&`）は許容される
    let source = r"\begin{table}\row{A & & C}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let DocNode::Table { rows, .. } = &result[0] else {
      panic!("Table が期待されます");
    };
    assert_eq!(rows[0].cells.len(), 3);
    assert!(rows[0].cells[1].content.is_empty());
  }
}
