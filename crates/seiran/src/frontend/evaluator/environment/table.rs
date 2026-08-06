//! 表環境 — `table`
//!
//! `\head`、`\row`、`\caption` を [`HirNodeKind::Table`] に変換する。

mod body;
mod cell;
mod opts;

use body::{resolve_column_count, scan_table_body};
use opts::{collect_table_opts, parse_columns_spec, parse_widths_spec};

use crate::{
  frontend::{evaluator::EvalError, span_ext::ToSourceSpan, syntax::ast::EnvironmentView},
  model::{ColumnAlign, ColumnWidth, HirBuilder, HirNode, HirNodeKind},
};

/// `table` 環境を評価する
///
/// `columns` と `widths` を列数に正規化し、各行のセル数を検証する。
///
/// # Errors
///
/// 未知の任意引数キー、揃え / 幅トークンの不正、セル数の不一致、
/// `\row` の欠如などが発生した場合にエラーを返します。
pub(super) fn table(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let opts = collect_table_opts(view)?;

  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "table".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let columns_tokens = opts.columns_spec.as_deref().map(|s| return parse_columns_spec(s, view)).transpose()?;
  let widths_tokens = opts.widths_spec.as_deref().map(|s| return parse_widths_spec(s, view)).transpose()?;

  let id = builder.alloc(view.span());
  let body = scan_table_body(view, builder)?;

  if body.head.is_empty() && body.rows.is_empty() {
    return Err(EvalError::MissingEnvironmentArgument {
      name: "table".to_string(),
      expected: "\\row コマンド".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let column_count =
    resolve_column_count(columns_tokens.as_deref(), widths_tokens.as_deref(), &body.head, &body.rows, view)?;

  let columns = columns_tokens.unwrap_or_else(|| vec![ColumnAlign::Left; column_count]);
  let widths = widths_tokens.unwrap_or_else(|| vec![ColumnWidth::Auto; column_count]);

  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::Table {
      columns,
      widths,
      head: body.head.into_iter().map(|(row, _)| return row).collect(),
      rows: body.rows.into_iter().map(|(row, _)| return row).collect(),
      caption: body.caption,
      caption_position: body.caption_position,
      label: opts.label,
      breakable: opts.breakable,
    },
  )]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::evaluator::{evaluate_children_to_hir, lookup_env_parse_mode},
    model::{CaptionPosition, HirInline, HirInlineKind, HirTableRow},
  };

  /// テスト用 `parse` ラッパ
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// ソースを評価して最初の `HirNodeKind::Table` を取り出すヘルパ
  fn eval_table(source: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let cst = parse(source, &arena).unwrap();
    return evaluate_children_to_hir(source, cst);
  }

  /// HIR インラインをプレーンテキストへ変換するテスト専用ヘルパ（`inline_nodes_to_plain_text` の HIR 版）
  ///
  /// スタイル情報を無視して文字列を連結する。`\ref` は本テストでは解決しないため空文字列扱い。
  fn hir_inlines_to_plain_text(inlines: &[HirInline]) -> String {
    let mut out = String::new();
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Text(s) => out.push_str(s),
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. } => out.push_str(&hir_inlines_to_plain_text(children)),
        HirInlineKind::InlineMath(_) => out.push_str("[Math]"),
        HirInlineKind::Symbol(ch) => out.push(*ch),
        HirInlineKind::LineBreak => out.push('\n'),
        HirInlineKind::NoIndent
        | HirInlineKind::Footnote { .. }
        | HirInlineKind::Index { .. }
        | HirInlineKind::Ref { .. } => {},
        HirInlineKind::Cite { keys } => out.push_str(&keys.join(", ")),
      }
    }
    return out;
  }

  /// セル内容のプレーンテキストを行ごとに並べるヘルパ
  fn row_texts(rows: &[HirTableRow]) -> Vec<Vec<String>> {
    return rows
      .iter()
      .map(|row| return row.cells.iter().map(|cell| return hir_inlines_to_plain_text(&cell.content)).collect())
      .collect();
  }

  #[test]
  fn table_extracts_head_rows_and_caption() {
    // Arrange
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
    let HirNodeKind::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      label,
      breakable,
      ..
    } = &result[0].kind
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
    assert_eq!(hir_inlines_to_plain_text(caption), "得点表");
    assert_eq!(*caption_position, CaptionPosition::Bottom);
    assert!(label.is_none());
    assert!(*breakable);
  }

  #[test]
  fn table_caption_before_rows_yields_top_position() {
    // Arrange
    let source = r"\begin{table}\caption{c}\row{A & B}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table {
      caption_position, ..
    } = &result[0].kind
    else {
      panic!("Table が期待されます");
    };
    assert_eq!(*caption_position, CaptionPosition::Top);
  }

  #[test]
  fn table_infers_column_count_from_rows() {
    // Arrange
    let source = r"\begin{table}\row{A & B & C}\row{D & E & F}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table {
      columns, widths, ..
    } = &result[0].kind
    else {
      panic!("Table が期待されます");
    };
    assert_eq!(columns, &[ColumnAlign::Left; 3]);
    assert_eq!(widths, &[ColumnWidth::Auto; 3]);
  }

  #[test]
  fn table_cell_span_counts_toward_column_count() {
    // Arrange
    let source = r"\begin{table}\row{A & B & C}\row[rule_above]{\cell[span=2]{合計} & 180}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table { rows, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].rule_above);
    assert!(rows[1].rule_above);
    assert_eq!(rows[1].cells.len(), 2);
    assert_eq!(rows[1].cells[0].span, 2);
    assert_eq!(hir_inlines_to_plain_text(&rows[1].cells[0].content), "合計");
    assert_eq!(rows[1].cells[1].span, 1);
    assert_eq!(hir_inlines_to_plain_text(&rows[1].cells[1].content), "180");
  }

  #[test]
  fn table_rejects_row_cell_count_mismatch() {
    // Arrange
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
    // Arrange
    let source = r#"\begin{table}[columns="l r"]\row{A & B}\end{table}"#;

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "columns"));
  }

  #[test]
  fn table_parses_width_ratio_and_flex() {
    // Arrange
    let source = r#"\begin{table}[widths="0.3 * auto"]\row{A & B & C}\end{table}"#;

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table { widths, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert!(matches!(widths[0], ColumnWidth::Ratio(r) if (r - 0.3).abs() < 1e-6));
    assert!(matches!(widths[1], ColumnWidth::Flex));
    assert!(matches!(widths[2], ColumnWidth::Auto));
  }

  #[test]
  fn table_rejects_invalid_width_token() {
    // Arrange
    let source = r#"\begin{table}[widths="1.5 auto"]\row{A & B}\end{table}"#;

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "widths"));
  }

  #[test]
  fn table_rejects_missing_rows() {
    // Arrange
    let source = r"\begin{table}\caption{c}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::MissingEnvironmentArgument { ref name, .. }) if name == "table"));
  }

  #[test]
  fn table_rejects_mixed_cell_and_text_in_segment() {
    // Arrange
    let source = r"\begin{table}\row{\cell[span=2]{合計} extra & 180}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::TableCellMixedContent { .. })));
  }

  #[test]
  fn table_rejects_rowspan_key() {
    // Arrange
    let source = r"\begin{table}\row{\cell[rowspan=2]{X} & Y}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "rowspan"));
  }

  #[test]
  fn table_rejects_line_break_in_cell() {
    // Arrange
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
    // Arrange
    let source = r"\begin{table}\head{stray \row{A}}\row{B}\end{table}";

    // Act
    let result = eval_table(source);

    // Assert
    assert!(matches!(result, Err(EvalError::UnexpectedContentInEnvironment { ref env, .. }) if env == "table"));
  }

  #[test]
  fn table_captures_label() {
    // Arrange
    let source = r"\begin{table}[label=tab:a]\row{A}\end{table}\begin{table}\row{B}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let HirNodeKind::Table { label, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert_eq!(label.as_deref(), Some("tab:a"));
    let HirNodeKind::Table { label, .. } = &result[1].kind else {
      panic!("Table が期待されます");
    };
    assert!(label.is_none());
  }

  #[test]
  fn table_breakable_false_is_captured() {
    // Arrange
    let source = r"\begin{table}[breakable=false]\row{A & B}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table { breakable, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert!(!breakable);
  }

  #[test]
  fn table_cell_content_is_trimmed() {
    // Arrange
    let source = r"\begin{table}\row{  Alice   &   92  }\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table { rows, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert_eq!(row_texts(rows), vec![vec!["Alice", "92"]]);
  }

  #[test]
  fn table_cell_preserves_inline_styles() {
    // Arrange
    let source = r"\begin{table}\row{\bold{強調} & $x^2$}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table { rows, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert!(matches!(
      &rows[0].cells[0].content[0].kind,
      HirInlineKind::Styled {
        kind: crate::model::FontKind::SerifBold,
        ..
      }
    ));
    assert!(matches!(&rows[0].cells[1].content[0].kind, HirInlineKind::InlineMath(_)));
  }

  #[test]
  fn table_empty_cell_is_allowed() {
    // Arrange
    let source = r"\begin{table}\row{A & & C}\end{table}";

    // Act
    let result = eval_table(source).unwrap();

    // Assert
    let HirNodeKind::Table { rows, .. } = &result[0].kind else {
      panic!("Table が期待されます");
    };
    assert_eq!(rows[0].cells.len(), 3);
    assert!(rows[0].cells[1].content.is_empty());
  }
}
