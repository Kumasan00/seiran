//! 表環境（`resolve::ResolvedNode::Table`）の lowering

use model::{CaptionPosition, ColumnAlign, ColumnWidth, FontKind, TableColumn};
use resolve::{ResolvedInline, ResolvedTableRow};

use super::{
  LoweringContext, LoweringState,
  float::{FloatSpec, build_caption, wrap_float},
  inline::lower_inline,
  layout_node::{LayoutNode, TableCellLayout, TableLayout, TableRowLayout, TextStyle},
};

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

/// `ResolvedTableRow` の列を [`TableRowLayout`] の列に変換する
fn lower_rows(
  ctx: &LoweringContext,
  rows: &[ResolvedTableRow],
  cell_style: TextStyle,
  state: &mut LoweringState,
) -> Vec<TableRowLayout> {
  let mut result = Vec::with_capacity(rows.len());
  for row in rows {
    let mut cells = Vec::with_capacity(row.cells.len());
    for cell in &row.cells {
      let mut content = Vec::new();
      for inline in &cell.content {
        content.extend(lower_inline(ctx, inline, cell_style, state));
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
  return result;
}

/// 表をレイアウトノードに変換する
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_table(
  ctx: &LoweringContext,
  columns: &[ColumnAlign],
  widths: &[ColumnWidth],
  head: &[ResolvedTableRow],
  rows: &[ResolvedTableRow],
  caption: Option<(CaptionPosition, &[ResolvedInline])>,
  number: &str,
  breakable: bool,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let style = &ctx.style.table;

  let body_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.style.text.font_kind,
    color: None,
  };
  let head_style = TextStyle {
    font_size: body_style.font_size,
    font_kind: bold_kind(body_style.font_kind),
    color: None,
  };

  let table_node = LayoutNode::Table(TableLayout {
    columns: columns
      .iter()
      .zip(widths)
      .map(|(align, width)| {
        return TableColumn {
          align: *align,
          width: *width,
        };
      })
      .collect(),
    head: lower_rows(ctx, head, head_style, state),
    rows: lower_rows(ctx, rows, body_style, state),
    breakable,
  });

  let caption_nodes =
    caption.map(|(position, inlines)| return (position, build_caption(ctx, &style.caption, inlines, number, state)));
  let spec = FloatSpec {
    top_margin: style.top_margin,
    bottom_margin: style.bottom_margin,
    inner_margin: style.inner_margin,
  };
  return wrap_float(table_node, caption_nodes, &spec);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use config::Style as ReadStyle;
  use resolve::ResolvedTableCell;

  use super::{super::test_support, *};

  /// 1 セルの `ResolvedTableRow` を作るテスト用ヘルパ
  fn row_of(texts: &[&str]) -> ResolvedTableRow {
    return ResolvedTableRow {
      cells: texts
        .iter()
        .map(|t| {
          return ResolvedTableCell {
            content: vec![ResolvedInline::Text((*t).to_string())],
            span: 1,
          };
        })
        .collect(),
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
        LayoutNode::Table(t) => return Some(t),
        _ => return None,
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
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
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
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left],
      &[ColumnWidth::Auto],
      &head,
      &rows,
      None,
      "1",
      true,
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
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
    let caption = [ResolvedInline::Text("得点表".to_string())];

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
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
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
    let caption = [ResolvedInline::Text("得点表".to_string())];

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
      &mut LoweringState::new(&test_support::document(&[])),
    );

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
  fn lower_table_inserts_inner_margin_between_table_and_caption() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [row_of(&["A"])];
    let caption = [ResolvedInline::Text("得点表".to_string())];

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
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待されます");
    };
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let inner_kern = children.get(table_idx + 1);
    assert!(
      matches!(inner_kern, Some(LayoutNode::Vkern { length }) if (length.to_pt() - style.table.inner_margin.to_pt()).abs() < f32::EPSILON),
      "本体の直後に inner_margin の Vkern が入る: {children:?}"
    );
  }

  #[test]
  fn bold_kind_maps_regular_to_bold() {
    assert_eq!(bold_kind(FontKind::Serif), FontKind::SerifBold);
    assert_eq!(bold_kind(FontKind::SansSerifItalic), FontKind::SansSerifBoldItalic);
    assert_eq!(bold_kind(FontKind::Monospace), FontKind::MonospaceBold);
    assert_eq!(bold_kind(FontKind::SerifBold), FontKind::SerifBold);
  }

  #[test]
  fn lower_table_preserves_column_widths() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [row_of(&["a", "b", "c"])];
    let widths = [
      ColumnWidth::Fixed(model::Length::pt(40.0)),
      ColumnWidth::Ratio(0.25),
      ColumnWidth::Flex,
    ];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right],
      &widths,
      &[],
      &rows,
      None,
      "1",
      true,
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let table = find_table(&nodes);
    assert_eq!(table.columns.len(), 3);
    assert!(matches!(table.columns[0].width, ColumnWidth::Fixed(l) if (l.to_pt() - 40.0).abs() < f32::EPSILON));
    assert!(matches!(table.columns[1].width, ColumnWidth::Ratio(r) if (r - 0.25).abs() < f32::EPSILON));
    assert!(matches!(table.columns[2].width, ColumnWidth::Flex));
    assert_eq!(table.columns[1].align, ColumnAlign::Center);
  }

  #[test]
  fn lower_table_breakable_false_is_preserved() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [row_of(&["A"])];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left],
      &[ColumnWidth::Auto],
      &[],
      &rows,
      None,
      "1",
      false,
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let table = find_table(&nodes);
    assert!(!table.breakable);
  }

  #[test]
  fn lower_table_preserves_rule_above_flag() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [ResolvedTableRow {
      cells: vec![ResolvedTableCell {
        content: vec![ResolvedInline::Text("A".to_string())],
        span: 1,
      }],
      rule_above: true,
    }];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left],
      &[ColumnWidth::Auto],
      &[],
      &rows,
      None,
      "1",
      true,
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let table = find_table(&nodes);
    assert!(table.rows[0].rule_above);
  }

  #[test]
  fn lower_table_without_caption_omits_caption_text() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [row_of(&["A"])];

    // Act
    let nodes = lower_table(
      &ctx,
      &[ColumnAlign::Left],
      &[ColumnWidth::Auto],
      &[],
      &rows,
      None,
      "1",
      true,
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待されます");
    };
    let has_text = children.iter().any(|n| matches!(n, LayoutNode::Text(_, _)));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
  }

  #[test]
  fn lower_table_cell_footnote_shares_document_wide_counter() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = [ResolvedTableRow {
      cells: vec![ResolvedTableCell {
        content: vec![ResolvedInline::Footnote {
          body: vec![ResolvedInline::Text("cell note".to_string())],
          span: model::Span::DUMMY,
        }],
        span: 1,
      }],
      rule_above: false,
    }];
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    state.next_footnote_index(); // 本文側で既に 1 個採番済みという想定

    // Act
    let nodes = lower_table(&ctx, &[ColumnAlign::Left], &[ColumnWidth::Auto], &[], &rows, None, "1", true, &mut state);

    // Assert
    let table = find_table(&nodes);
    assert!(
      matches!(&table.rows[0].cells[0].content[1], LayoutNode::Footnote { number: 2, .. }),
      "{:?}",
      table.rows[0].cells[0].content
    );
  }
}
