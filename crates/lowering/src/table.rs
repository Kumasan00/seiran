//! 表環境（`DocNode::Table`）の lowering
//!
//! セル内容を [`crate::inline::lower_inline`] でスタイル付きレイアウトノードに変換し、
//! [`TableLayout`] に詰めます。ヘッダ行のセルは本文フォントの太字バリアントで
//! 描画されます。キャプションは `TableStyle.caption` の書式で構築し、図と同様に
//! `caption_position`（ソース上の `\caption` の出現順）に従って表の上下に配置します。
//! キャプション構築と `VBox` 包みは [`crate::float`] の共通ヘルパで行います。
//!
//! 列幅の解決（自然幅の実測・残余分配）はシェーピング結果が必要なため
//! `pdf_gen` 段で行い、ここでは列指定をそのまま保持します。

use parser::document::{CaptionPosition, InlineNode, TableRow};
use types::{ColumnAlign, ColumnWidth, FontKind};

use super::{
  LoweringContext, LoweringError,
  float::{FloatSpec, build_caption, wrap_float},
  inline::lower_inline,
};
use crate::layout_node::{LayoutNode, TableCellLayout, TableColumn, TableLayout, TableRowLayout, TextStyle};

/// 本文用の `FontKind` を太字バリアントに変換する（ヘッダ行セル用）
fn bold_kind(kind: FontKind) -> FontKind {
  return match kind {
    FontKind::Serif => FontKind::SerifBold,
    FontKind::SerifItalic => FontKind::SerifBoldItalic,
    FontKind::SansSerif => FontKind::SansSerifBold,
    FontKind::SansSerifItalic => FontKind::SansSerifBoldItalic,
    FontKind::Monospace => FontKind::MonospaceBold,
    FontKind::MonospaceItalic => FontKind::MonospaceBoldItalic,
    other => other,
  };
}

/// `TableRow` の列を [`TableRowLayout`] の列に変換する
fn lower_rows(
  ctx: &LoweringContext,
  rows: &[TableRow],
  cell_style: TextStyle,
) -> Result<Vec<TableRowLayout>, LoweringError> {
  let mut result = Vec::with_capacity(rows.len());
  for row in rows {
    let mut cells = Vec::with_capacity(row.cells.len());
    for cell in &row.cells {
      let mut content = Vec::new();
      for inline in &cell.content {
        content.extend(lower_inline(ctx, inline, cell_style)?);
      }
      cells.push(TableCellLayout {
        content,
        span: cell.span,
      });
    }
    result.push(TableRowLayout {
      cells,
      rule_above: row.rule_above,
    });
  }
  return Ok(result);
}

/// 表をレイアウトノードに変換する
///
/// 表本体とキャプションを `caption_position` の順序で積み、上下マージン付きの
/// `VBox` で囲んで返す。`caption` が `None` のときはキャプション行を出力しない。
///
/// # Errors
///
/// キャプション・セル内に未解決の `\ref` がある場合に
/// [`LoweringError::UnresolvedReference`] を返します。
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_table(
  ctx: &LoweringContext,
  columns: &[ColumnAlign],
  widths: &[ColumnWidth],
  head: &[TableRow],
  rows: &[TableRow],
  caption: Option<(CaptionPosition, &[InlineNode])>,
  number: &str,
  breakable: bool,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let style = &ctx.style.core.table;

  let body_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.style.core.text.font_kind,
  };
  let head_style = TextStyle {
    font_size: body_style.font_size,
    font_kind: bold_kind(body_style.font_kind),
  };

  let table_node = LayoutNode::Table(TableLayout {
    columns: columns
      .iter()
      .zip(widths)
      .map(|(align, width)| TableColumn {
        align: *align,
        width: *width,
      })
      .collect(),
    head: lower_rows(ctx, head, head_style)?,
    rows: lower_rows(ctx, rows, body_style)?,
    breakable,
  });

  let caption_nodes = match caption {
    Some((position, inlines)) => Some((position, build_caption(ctx, &style.caption, inlines, number)?)),
    None => None,
  };
  let spec = FloatSpec {
    top_margin: style.top_margin,
    bottom_margin: style.bottom_margin,
    inner_margin: None,
    break_after_main: false,
  };
  return Ok(wrap_float(table_node, caption_nodes, &spec));
}

#[cfg(test)]
mod tests {
  use parser::document::{InlineNode, TableCell};
  use read_style::Style as ReadStyle;

  use super::*;

  /// 1 セルの `TableRow` を作るテスト用ヘルパ
  fn row_of(texts: &[&str]) -> TableRow {
    return TableRow {
      cells: texts.iter().map(|t| TableCell::new(vec![InlineNode::Text((*t).to_string())])).collect(),
      rule_above: false,
    };
  }

  /// `lower_table` の結果から `TableLayout` を取り出すヘルパ
  fn find_table(nodes: &[LayoutNode]) -> &TableLayout {
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    return children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Table(t) => Some(t),
        _ => None,
      })
      .expect("VBox 内に Table があるはず");
  }

  #[test]
  fn lower_table_builds_columns_and_rows() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let head = [row_of(&["Name", "Score"])];
    let rows = [row_of(&["Alice", "92"]), row_of(&["Bob", "88"])];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left, ColumnAlign::Right],
      &[ColumnWidth::Auto, ColumnWidth::Auto],
      &head,
      &rows,
      None,
      "1",
      true,
    )
    .expect("解決済みインラインなのでエラーにならない");

    // Assert — 先頭は top_margin の Vkern、VBox 内に Table
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })));
    let table = find_table(&nodes);
    assert_eq!(table.columns.len(), 2);
    assert_eq!(table.columns[0].align, ColumnAlign::Left);
    assert_eq!(table.columns[1].align, ColumnAlign::Right);
    assert_eq!(table.head.len(), 1);
    assert_eq!(table.rows.len(), 2);
    assert!(table.breakable);
  }

  #[test]
  fn lower_table_head_cells_use_bold_font() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let head = [row_of(&["Name"])];
    let rows = [row_of(&["Alice"])];

    // Act
    let nodes =
      lower_table(&ctx, &[ColumnAlign::Left], &[ColumnWidth::Auto], &head, &rows, None, "1", true).expect("失敗しない");

    // Assert — ヘッダセルは太字、本体セルは通常
    let table = find_table(&nodes);
    let LayoutNode::Text(_, head_style) = &table.head[0].cells[0].content[0] else {
      panic!("ヘッダセルは Text であるべき");
    };
    assert_eq!(head_style.font_kind, FontKind::SerifBold);
    let LayoutNode::Text(_, body_style) = &table.rows[0].cells[0].content[0] else {
      panic!("本体セルは Text であるべき");
    };
    assert_eq!(body_style.font_kind, FontKind::Serif);
  }

  #[test]
  fn lower_table_caption_bottom_places_caption_after_table() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [row_of(&["A"])];
    let caption = [InlineNode::Text("得点表".to_string())];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left],
      &[ColumnWidth::Auto],
      &[],
      &rows,
      Some((CaptionPosition::Bottom, &caption)),
      "1",
      true,
    )
    .expect("失敗しない");

    // Assert — VBox 内で Table がキャプション Text より前、書式は "Table {number}: {title}"
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待されます");
    };
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let caption_idx = children
      .iter()
      .position(|n| matches!(n, LayoutNode::Text(t, _) if t == "Table 1: 得点表"))
      .expect("キャプション Text あり");
    assert!(table_idx < caption_idx, "Bottom: table がキャプションの前");
  }

  #[test]
  fn lower_table_caption_top_places_caption_before_table() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [row_of(&["A"])];
    let caption = [InlineNode::Text("得点表".to_string())];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left],
      &[ColumnWidth::Auto],
      &[],
      &rows,
      Some((CaptionPosition::Top, &caption)),
      "2",
      true,
    )
    .expect("失敗しない");

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待されます");
    };
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let caption_idx = children
      .iter()
      .position(|n| matches!(n, LayoutNode::Text(t, _) if t.starts_with("Table 2")))
      .expect("キャプション Text あり");
    assert!(caption_idx < table_idx, "Top: キャプションが table の前");
  }

  #[test]
  fn bold_kind_maps_regular_to_bold() {
    assert_eq!(bold_kind(FontKind::Serif), FontKind::SerifBold);
    assert_eq!(bold_kind(FontKind::SansSerifItalic), FontKind::SansSerifBoldItalic);
    assert_eq!(bold_kind(FontKind::Monospace), FontKind::MonospaceBold);
    // 既に太字のものはそのまま
    assert_eq!(bold_kind(FontKind::SerifBold), FontKind::SerifBold);
  }
}
