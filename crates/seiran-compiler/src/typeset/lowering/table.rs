//! 表環境（`document::HirNodeKind::Table`）の lowering

use crate::{
  document::{CaptionPosition, ColumnAlign, ColumnWidth, FontKind, HirInline, HirTableRow},
  typeset::{
    boxes::TableColumn,
    lowering::{
      LoweringContext, LoweringState,
      float::{FloatSpec, build_caption, wrap_float},
      inline::lower_inlines,
      layout_node::{LayoutNode, TableCellLayout, TableLayout, TableRowLayout, TextStyle},
    },
  },
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

/// `HirTableRow` の列を [`TableRowLayout`] の列に変換する
fn lower_rows(
  ctx: &LoweringContext,
  rows: &[HirTableRow],
  cell_style: TextStyle,
  state: &mut LoweringState,
) -> Vec<TableRowLayout> {
  let mut result = Vec::with_capacity(rows.len());
  for row in rows {
    let mut cells = Vec::with_capacity(row.cells.len());
    for cell in &row.cells {
      cells.push(TableCellLayout {
        content: lower_inlines(ctx, &cell.content, cell_style, state),
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
#[allow(
  clippy::too_many_arguments,
  reason = "表 1 件の lowering に要る値を束ねる中間型を作っても、呼び出し側が同じ数の値を詰め替えるだけになる"
)]
pub(super) fn lower_table(
  ctx: &LoweringContext,
  columns: &[ColumnAlign],
  widths: &[ColumnWidth],
  head: &[HirTableRow],
  rows: &[HirTableRow],
  caption: Option<(CaptionPosition, &[HirInline])>,
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
mod tests {
  use super::{
    super::test_support::{analyzed, lower},
    *,
  };
  use crate::{length::Length, style::Style as ReadStyle};

  /// `.sei` ソースを lower してレイアウトノード列を返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  /// 表の本体 `VBox` の子要素列を取り出すヘルパ
  fn table_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } if children.iter().any(|c| matches!(c, LayoutNode::Table(_))) => {
          return Some(children.as_slice());
        },
        _ => return None,
      })
      .expect("表本体の VBox があるはず");
  }

  /// `lower_table` の結果から `TableLayout` を取り出すヘルパ
  fn find_table(nodes: &[LayoutNode]) -> &TableLayout {
    return table_children(nodes)
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

    // Act
    let nodes = lower_source(
      &style,
      "\\begin{table}[columns=\"left right\"]\n\\head{\n\\row{Name & Score}\n}\n\
       \\row{Alice & 92}\n\\row{Bob & 88}\n\\end{table}\n",
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

    // Act
    let nodes = lower_source(&style, "\\begin{table}\n\\head{\n\\row{Name}\n}\n\\row{Alice}\n\\end{table}\n");

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

    // Act — `\caption` を行より後ろに置くとキャプションは表の下になる
    let nodes = lower_source(&style, "\\chapter{C}\n\n\\begin{table}\n\\row{A}\n\\caption{得点表}\n\\end{table}\n");

    // Assert
    let children = table_children(&nodes);
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let caption_idx = children
      .iter()
      .position(|n| matches!(n, LayoutNode::Text(t, _) if t == "Table 1.1: 得点表"))
      .expect("キャプション Text あり");
    assert!(table_idx < caption_idx, "Bottom: table がキャプションの前");
  }

  #[test]
  fn lower_table_caption_top_places_caption_before_table() {
    // Arrange
    let style = ReadStyle::default();

    // Act — `\caption` を行より前に置くとキャプションは表の上になる
    let nodes = lower_source(&style, "\\chapter{C}\n\n\\begin{table}\n\\caption{得点表}\n\\row{A}\n\\end{table}\n");

    // Assert
    let children = table_children(&nodes);
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let caption_idx = children
      .iter()
      .position(|n| matches!(n, LayoutNode::Text(t, _) if t.starts_with("Table 1.1")))
      .expect("キャプション Text あり");
    assert!(caption_idx < table_idx, "Top: キャプションが table の前");
  }

  #[test]
  fn lower_table_inserts_inner_margin_between_table_and_caption() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\chapter{C}\n\n\\begin{table}\n\\row{A}\n\\caption{得点表}\n\\end{table}\n");

    // Assert
    let children = table_children(&nodes);
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

    // Act
    let nodes = lower_source(
      &style,
      "\\begin{table}[columns=\"left center right\", widths=\"40mm 0.25 *\"]\n\\row{a & b & c}\n\\end{table}\n",
    );

    // Assert
    let table = find_table(&nodes);
    assert_eq!(table.columns.len(), 3);
    assert!(
      matches!(table.columns[0].width, ColumnWidth::Fixed(l) if (l.to_pt() - Length::mm(40.0).to_pt()).abs() < f32::EPSILON)
    );
    assert!(matches!(table.columns[1].width, ColumnWidth::Ratio(r) if (r - 0.25).abs() < f32::EPSILON));
    assert!(matches!(table.columns[2].width, ColumnWidth::Flex));
    assert_eq!(table.columns[1].align, ColumnAlign::Center);
  }

  #[test]
  fn lower_table_breakable_false_is_preserved() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{table}[breakable=false]\n\\row{A}\n\\end{table}\n");

    // Assert
    let table = find_table(&nodes);
    assert!(!table.breakable);
  }

  #[test]
  fn lower_table_preserves_rule_above_flag() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{table}\n\\row[rule_above]{A}\n\\end{table}\n");

    // Assert
    let table = find_table(&nodes);
    assert!(table.rows[0].rule_above);
  }

  #[test]
  fn lower_table_without_caption_omits_caption_text() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{table}\n\\row{A}\n\\end{table}\n");

    // Assert
    let children = table_children(&nodes);
    let has_text = children.iter().any(|n| matches!(n, LayoutNode::Text(_, _)));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
  }

  #[test]
  fn lower_table_cell_footnote_shares_document_wide_counter() {
    // Arrange — 本文側で 1 個採番したあとに、表セルの脚注が 2 番になることを見る
    let style = ReadStyle::default();

    // Act
    let nodes =
      lower_source(&style, "本文\\footnote{body note}\n\n\\begin{table}\n\\row{\\footnote{cell note}}\n\\end{table}\n");

    // Assert
    let table = find_table(&nodes);
    assert!(
      matches!(&table.rows[0].cells[0].content[1], LayoutNode::Footnote { number: 2, .. }),
      "{:?}",
      table.rows[0].cells[0].content
    );
  }
}
