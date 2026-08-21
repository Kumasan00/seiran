//! シェーピング済みの表ボックスと計測関数。
//!
//! セル内容は計測済みの [`HItem`] 列として保持されるため、列幅・行高・セル内 x 座標の算出は
//! フォントに触れない純粋関数として本モジュールで提供する。`breaking::break_pages` がページ上の
//! 絶対座標へ確定し、描画 adapter は確定値を型変換するだけになる。

use super::{
  hitem::{HBoxContent, HItem},
  line::PositionedBox,
  link::LinkTarget,
};
use crate::{
  document::{ColumnAlign, ColumnWidth},
  length::Length,
};

/// 表の 1 列の定義（揃え + 幅指定）
///
/// 著者が書いた `columns=` / `widths=` は HIR では別々の列（[`ColumnAlign`] / [`ColumnWidth`]）で、
/// `typeset::lowering` が列ごとに 1 つへ束ねたものが本型。表レイアウトの入力契約なので後段の
/// layout が所有する（#334）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct TableColumn {
  /// セル内容の揃え方向
  pub align: ColumnAlign,
  /// 列幅の指定方法
  pub width: ColumnWidth,
}

/// 表ボックス（シェーピング済みの表全体）
#[derive(Debug, Clone)]
pub(crate) struct TableBox {
  /// 列の定義（揃え + 幅指定）。列数はこの長さで確定する
  pub columns: Vec<TableColumn>,
  /// ヘッダ行。改ページ時にページ先頭へ再描画される
  pub head: Vec<TableRowBox>,
  /// 本体行
  pub rows: Vec<TableRowBox>,
  /// 改ページによる分割を許可するか
  pub breakable: bool,
}

/// 表の 1 行（シェーピング済み）
#[derive(Debug, Clone)]
pub(crate) struct TableRowBox {
  /// 行内のセル
  pub cells: Vec<TableCellBox>,
  /// この行の上に横罫線を引くか
  pub rule_above: bool,
}

/// 表の 1 セル（シェーピング済み）
#[derive(Debug, Clone)]
pub(crate) struct TableCellBox {
  /// セル内容のアイテム列（`Box` / `Kern` / `Glue` が主だが、`\ref`/`\url`/`\href` を
  /// 含む場合は `LinkStart`/`LinkEnd` も現れる。行分割・ページ分割はセル内では無効）
  pub items: Vec<HItem>,
  /// 列方向の結合数（colspan、1 以上）
  pub span: u32,
}

/// アイテム列の自然幅を返す
///
/// box は計測済みの幅を持つため、フォントに触れずに合計できる。
#[must_use]
fn measure_items_width(items: &[HItem]) -> Length { return items.iter().map(HItem::natural_width).sum(); }

/// アイテム列に含まれるテキストの最大フォントサイズを返す（テキストがなければ `None`）
///
/// Atom（数式）の子要素も再帰的に走査する。
#[must_use]
pub(crate) fn max_font_size_in_items(items: &[HItem]) -> Option<Length> {
  return items
    .iter()
    .filter_map(|item| match item {
      HItem::Box(hbox) | HItem::FlushRight(hbox) => return max_font_size_in_content(&hbox.content),
      _ => return None,
    })
    .reduce(Length::max);
}

/// ボックス内容に含まれるテキストの最大フォントサイズを返す
fn max_font_size_in_content(content: &HBoxContent) -> Option<Length> {
  return match content {
    HBoxContent::Glyphs(run) => Some(run.font_size),
    HBoxContent::Rule { .. } => None,
    HBoxContent::Atom(children) => children
      .iter()
      .filter_map(|child| return max_font_size_in_content(&child.item.content))
      .reduce(Length::max),
  };
}

/// 行の高さ = 行内の最大フォントサイズ × 行高係数
#[must_use]
pub(crate) fn table_row_height(row: &TableRowBox, default_font_size: Length, line_height_factor: f32) -> Length {
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| return max_font_size_in_items(&cell.items))
    .reduce(Length::max)
    .unwrap_or(default_font_size);
  return max_font * line_height_factor;
}

/// 列幅を解決する
///
/// 1. `span = 1` のセルの自然幅（内容実測 + 左右 padding）で各列の自然幅を求める
/// 2. `span > 1` のセルは、跨ぐ列の自然幅合計が不足する場合に均等に加算する
/// 3. 列指定を適用する: `Fixed` は指定値、`Ratio` は本文幅比、`Auto` は自然幅、
///    `Flex`（`*`）は残り幅の等分（自然幅を下回る場合は自然幅）
///
/// 合計が本文幅を超える場合の縮小は行わない（セル折り返し未対応のため、はみ出しを許容する）。
#[must_use]
pub(crate) fn resolve_column_widths(table: &TableBox, available: Length, padding: Length) -> Vec<Length> {
  let column_count = table.columns.len();
  let mut naturals = vec![Length::ZERO; column_count];

  for row in table.head.iter().chain(table.rows.iter()) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let span = cell.span as usize;
      if span == 1 && column_index < column_count {
        let width = measure_items_width(&cell.items) + padding * 2;
        naturals[column_index] = naturals[column_index].max(width);
      }
      column_index += span;
    }
  }
  // span セルの不足分を跨ぐ列に均等配分する
  for row in table.head.iter().chain(table.rows.iter()) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let span = cell.span as usize;
      if span > 1 && column_index + span <= column_count {
        let width = measure_items_width(&cell.items) + padding * 2;
        let current: Length = naturals[column_index..column_index + span].iter().sum();
        if width > current {
          #[allow(clippy::cast_precision_loss, reason = "`span` は列数で、f32 の仮数部に収まる小さな整数")]
          let extra = (width - current) / span as f32;
          for natural in &mut naturals[column_index..column_index + span] {
            *natural += extra;
          }
        }
      }
      column_index += span;
    }
  }

  let mut widths = vec![Length::ZERO; column_count];
  let mut flex_indices: Vec<usize> = Vec::new();
  let mut used = Length::ZERO;
  for (i, column) in table.columns.iter().enumerate() {
    match column.width {
      ColumnWidth::Fixed(length) => {
        widths[i] = length;
        used += widths[i];
      },
      ColumnWidth::Ratio(ratio) => {
        widths[i] = available * ratio;
        used += widths[i];
      },
      ColumnWidth::Auto => {
        widths[i] = naturals[i].max(padding * 2);
        used += widths[i];
      },
      ColumnWidth::Flex => flex_indices.push(i),
    }
  }
  if !flex_indices.is_empty() {
    #[allow(clippy::cast_precision_loss, reason = "flex 列の本数は f32 の仮数部に収まる小さな整数")]
    let share = ((available - used) / flex_indices.len() as f32).max(Length::ZERO);
    for i in flex_indices {
      widths[i] = share.max(naturals[i]);
    }
  }
  return widths;
}

/// 1 行の各セルと内容開始位置（表左端からの相対 x）
struct CellPlacement<'a> {
  /// 対象のセル
  cell: &'a TableCellBox,
  /// 内容の開始位置（表左端からの相対 x。揃え + padding 適用済み）
  content_x: Length,
}

/// 1 行の各セルの帯位置・内容開始位置を順に計算する
///
/// 列揃え（[`ColumnAlign`]）と内側余白から `content_x` を求める。
#[must_use]
fn layout_row_cells<'a>(
  row: &'a TableRowBox,
  columns: &[TableColumn],
  col_widths: &[Length],
  padding: Length,
) -> Vec<CellPlacement<'a>> {
  let mut column_index = 0usize;
  let mut cell_x = Length::ZERO;
  let mut placements = Vec::with_capacity(row.cells.len());
  for cell in &row.cells {
    let span = (cell.span as usize).min(col_widths.len().saturating_sub(column_index));
    let band_width: Length = col_widths[column_index..column_index + span].iter().copied().sum();
    let content_width = measure_items_width(&cell.items);
    let align = columns.get(column_index).map_or(ColumnAlign::Left, |c| return c.align);
    let content_x = match align {
      ColumnAlign::Left => cell_x + padding,
      ColumnAlign::Center => cell_x + (band_width - content_width) / 2.0,
      ColumnAlign::Right => cell_x + band_width - padding - content_width,
    };
    placements.push(CellPlacement { cell, content_x });
    cell_x += band_width;
    column_index += span;
  }
  return placements;
}

/// 表の 1 行に含まれる描画対象の箱を、表左端からの相対 x 座標へ配置する
///
/// 列揃え・padding・`Kern` / `Glue` のカーソル前進はこの時点ですべて解決する。
/// リンク marker は [`collect_row_links`] が別に確定矩形へ変換するため描画箱には含めない。
/// セル内脚注・索引 marker は入力から到達可能だが、表セル内では本体を配置しないという
/// 現行制限を維持して描画箱を生成しない。
#[must_use]
pub(crate) fn position_table_row_boxes(
  row: &TableRowBox,
  columns: &[TableColumn],
  col_widths: &[Length],
  padding: Length,
) -> Vec<PositionedBox> {
  let mut boxes = Vec::new();
  for placement in layout_row_cells(row, columns, col_widths, padding) {
    let mut cursor = placement.content_x;
    for item in &placement.cell.items {
      match item {
        HItem::Box(hbox) => {
          boxes.push(PositionedBox {
            content: hbox.content.clone(),
            x: cursor,
            dy: Length::ZERO,
            width: hbox.width,
          });
          cursor += hbox.width;
        },
        HItem::Kern(value) => cursor += *value,
        HItem::Glue { natural, .. } => cursor += *natural,
        HItem::Penalty { .. }
        | HItem::Discretionary { .. }
        | HItem::ForcedBreak
        | HItem::LinkStart(_)
        | HItem::LinkEnd
        | HItem::FlushRight(_)
        | HItem::Footnote { .. }
        | HItem::IndexMark { .. } => {},
      }
    }
  }
  return boxes;
}

/// 表セル内のリンク領域（表左端からの相対座標）
#[derive(Debug)]
pub(crate) struct RowLink {
  /// リンクの行き先
  pub target: LinkTarget,
  /// 表左端からの相対な左端オフセット
  pub x0: Length,
  /// 表左端からの相対な右端オフセット
  pub x1: Length,
}

/// 1 行内のセルに含まれるリンク領域を、表左端からの相対 x0/x1 として収集する
///
/// セルは折り返さない（`TableCellBox.items` はフラットな未分割の水平アイテム列）ため、
/// `LinkStart`/`LinkEnd` は常に同一セル内で対応が閉じる。カーソル前進は
/// [`HItem::natural_width`] を使う。セル内脚注・索引 marker は幅 0 で、現行制限どおり
/// ページ上の脚注・索引としては配置しない。
#[must_use]
pub(crate) fn collect_row_links(
  row: &TableRowBox,
  columns: &[TableColumn],
  col_widths: &[Length],
  padding: Length,
) -> Vec<RowLink> {
  let mut links = Vec::new();
  for placement in layout_row_cells(row, columns, col_widths, padding) {
    let mut cursor = placement.content_x;
    let mut open: Vec<(LinkTarget, Length)> = Vec::new();
    for item in &placement.cell.items {
      match item {
        HItem::LinkStart(target) => open.push((target.clone(), cursor)),
        HItem::LinkEnd => {
          if let Some((target, x0)) = open.pop() {
            links.push(RowLink {
              target,
              x0,
              x1: cursor,
            });
          }
        },
        _ => cursor += item.natural_width(),
      }
    }
  }
  return links;
}

#[cfg(test)]
mod tests {
  use super::{
    super::{
      hitem::{HBox, HBoxContent, HItem, PlacedHItem},
      link::{AnchorId, LinkTarget},
    },
    TableBox, TableCellBox, TableColumn, TableRowBox, collect_row_links, max_font_size_in_items, measure_items_width,
    position_table_row_boxes, resolve_column_widths, table_row_height,
  };
  use crate::{
    document::{ColumnAlign, ColumnWidth},
    length::Length,
    project::FontType,
    semantics::LabelId,
    typeset::GlyphRun,
  };

  /// pt 値から `Length` を作る
  fn pt(value: f32) -> Length { return Length::pt(value); }

  #[test]
  fn table_column_holds_align_and_width() {
    // Arrange
    let col = TableColumn {
      align: ColumnAlign::Right,
      width: ColumnWidth::Ratio(0.25),
    };

    // Assert
    assert_eq!(col.align, ColumnAlign::Right);
    assert_eq!(col.width, ColumnWidth::Ratio(0.25));
  }

  /// `Length` が pt 値 `expected` に一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 指定幅・指定フォントサイズの Glyphs ボックスを作る
  fn glyph_box(width: f32, font_size: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: pt(font_size),
        text: "x".to_string(),
        glyphs: Vec::new(),
        font_type: FontType::Serif,
        color: None,
      }),
      width: pt(width),
      height: pt(font_size),
      depth: Length::ZERO,
    });
  }

  /// 指定幅の Rule ボックスを作る
  fn rule_box(width: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: pt(width),
        height: pt(1.0),
      },
      width: pt(width),
      height: pt(1.0),
      depth: Length::ZERO,
    });
  }

  /// `span=1` のセルを作る
  fn cell(items: Vec<HItem>) -> TableCellBox { return TableCellBox { items, span: 1 }; }

  /// `rule_above = false` の行を作る
  fn row(cells: Vec<TableCellBox>) -> TableRowBox {
    return TableRowBox {
      cells,
      rule_above: false,
    };
  }

  #[test]
  fn measure_items_width_is_additive() {
    // Arrange
    let items = vec![
      rule_box(10.0),
      HItem::Glue {
        natural: pt(5.0),
        stretch: Length::ZERO,
        shrink: Length::ZERO,
        breakable: true,
      },
      HItem::Kern(pt(3.0)),
    ];

    // Act / Assert
    assert!(close(measure_items_width(&items), 18.0));
    assert!(close(measure_items_width(&[HItem::Penalty { value: 0 }, HItem::ForcedBreak]), 0.0));
  }

  #[test]
  fn max_font_size_in_items_returns_largest_text_size() {
    // Arrange
    let items = vec![glyph_box(20.0, 10.0), rule_box(5.0), glyph_box(20.0, 14.0)];

    // Act / Assert
    assert_eq!(max_font_size_in_items(&items), Some(pt(14.0)));
  }

  #[test]
  fn max_font_size_in_items_none_without_text() {
    // Arrange / Act / Assert
    assert_eq!(max_font_size_in_items(&[rule_box(5.0)]), None);
  }

  #[test]
  fn max_font_size_in_items_recurses_into_atom() {
    // Arrange
    let inner = HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: pt(20.0),
        text: "y".to_string(),
        glyphs: Vec::new(),
        font_type: FontType::Math,
        color: None,
      }),
      width: pt(8.0),
      height: pt(20.0),
      depth: Length::ZERO,
    };
    let atom = HBox::atom(vec![PlacedHItem {
      item: inner,
      dy: Length::ZERO,
      dx: Length::ZERO,
    }]);

    // Act / Assert
    assert_eq!(max_font_size_in_items(&[HItem::Box(atom)]), Some(pt(20.0)));
  }

  #[test]
  fn table_row_height_is_max_font_times_factor() {
    // Arrange
    let row = row(vec![
      cell(vec![glyph_box(10.0, 10.0)]),
      cell(vec![glyph_box(10.0, 12.0)]),
    ]);

    // Act / Assert
    assert!(close(table_row_height(&row, pt(9.0), 1.5), 18.0));
  }

  #[test]
  fn table_row_height_falls_back_to_default_font() {
    // Arrange
    let row = row(vec![cell(vec![rule_box(5.0)])]);

    // Act / Assert
    assert!(close(table_row_height(&row, pt(9.0), 2.0), 18.0));
  }

  #[test]
  fn resolve_column_widths_fixed_auto_ratio() {
    // Arrange
    let table = TableBox {
      columns: vec![
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Fixed(Length::pt(40.0)),
        },
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Auto,
        },
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Ratio(0.5),
        },
      ],
      head: Vec::new(),
      rows: vec![row(vec![
        cell(vec![rule_box(5.0)]),
        cell(vec![rule_box(30.0)]),
        cell(vec![rule_box(5.0)]),
      ])],
      breakable: true,
    };

    // Act
    let widths = resolve_column_widths(&table, pt(200.0), pt(2.0));

    // Assert
    assert!(close(widths[0], 40.0), "Fixed: {widths:?}");
    assert!(close(widths[1], 34.0), "Auto=内容+2*padding: {widths:?}");
    assert!(close(widths[2], 100.0), "Ratio=比*available: {widths:?}");
  }

  #[test]
  fn resolve_column_widths_flex_shares_remaining() {
    // Arrange
    let table = TableBox {
      columns: vec![
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Auto,
        },
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Flex,
        },
      ],
      head: Vec::new(),
      rows: vec![row(vec![
        cell(vec![rule_box(20.0)]),
        cell(vec![rule_box(10.0)]),
      ])],
      breakable: true,
    };

    // Act
    let widths = resolve_column_widths(&table, pt(100.0), pt(0.0));

    // Assert
    assert!(close(widths[0], 20.0), "{widths:?}");
    assert!(close(widths[1], 80.0), "Flex=残り幅の等分: {widths:?}");
  }

  #[test]
  fn resolve_column_widths_flex_never_below_natural() {
    // Arrange
    let table = TableBox {
      columns: vec![
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Auto,
        },
        TableColumn {
          align: ColumnAlign::Left,
          width: ColumnWidth::Flex,
        },
      ],
      head: Vec::new(),
      rows: vec![row(vec![
        cell(vec![rule_box(90.0)]),
        cell(vec![rule_box(40.0)]),
      ])],
      breakable: true,
    };

    // Act
    let widths = resolve_column_widths(&table, pt(100.0), pt(0.0));

    // Assert
    assert!(close(widths[1], 40.0), "Flex は自然幅を下回らない: {widths:?}");
  }

  /// 左揃え 2 列（幅 50/50）の共通カラム定義
  fn two_left_columns() -> Vec<TableColumn> {
    return vec![
      TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      },
      TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      },
    ];
  }

  #[test]
  fn collect_row_links_single_link_fills_cell() {
    // Arrange
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(target.clone()),
      rule_box(20.0),
      HItem::LinkEnd,
    ])]);
    let col_widths = vec![pt(30.0)];

    // Act
    let links = collect_row_links(
      &row,
      &[TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      &col_widths,
      pt(2.0),
    );

    // Assert
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x0, 2.0));
    assert!(close(links[0].x1, 22.0));
  }

  #[test]
  fn position_table_row_boxes_resolves_alignment_and_spacing() {
    // Arrange
    let row = row(vec![cell(vec![
      rule_box(5.0),
      HItem::Kern(pt(3.0)),
      HItem::Glue {
        natural: pt(2.0),
        stretch: Length::ZERO,
        shrink: Length::ZERO,
        breakable: false,
      },
      rule_box(4.0),
    ])]);
    let columns = vec![TableColumn {
      align: ColumnAlign::Right,
      width: ColumnWidth::Auto,
    }];

    // Act
    let boxes = position_table_row_boxes(&row, &columns, &[pt(30.0)], pt(2.0));

    // Assert
    assert_eq!(boxes.len(), 2);
    assert!(close(boxes[0].x, 14.0), "右揃えの先頭 x: {boxes:?}");
    assert!(close(boxes[1].x, 24.0), "box + kern + glue 後の x: {boxes:?}");
  }

  #[test]
  fn collect_row_links_multiple_links_in_one_cell() {
    // Arrange
    let first = LinkTarget::Internal(AnchorId::Label(LabelId::new("fig:1")));
    let second = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(first.clone()),
      rule_box(5.0),
      HItem::LinkEnd,
      rule_box(3.0),
      HItem::LinkStart(second.clone()),
      rule_box(5.0),
      HItem::LinkEnd,
    ])]);
    let col_widths = vec![pt(30.0)];
    let columns = vec![TableColumn {
      align: ColumnAlign::Left,
      width: ColumnWidth::Auto,
    }];

    // Act
    let links = collect_row_links(&row, &columns, &col_widths, pt(0.0));

    // Assert
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].target, first);
    assert!(close(links[0].x0, 0.0) && close(links[0].x1, 5.0));
    assert_eq!(links[1].target, second);
    assert!(close(links[1].x0, 8.0) && close(links[1].x1, 13.0));
  }

  #[test]
  fn collect_row_links_in_spanned_cell() {
    // Arrange
    let target = LinkTarget::Internal(AnchorId::Label(LabelId::new("tab:x")));
    let row = row(vec![TableCellBox {
      items: vec![
        HItem::LinkStart(target.clone()),
        rule_box(10.0),
        HItem::LinkEnd,
      ],
      span: 2,
    }]);
    let col_widths = vec![pt(20.0), pt(20.0)];

    // Act
    let links = collect_row_links(&row, &two_left_columns(), &col_widths, pt(0.0));

    // Assert
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x0, 0.0) && close(links[0].x1, 10.0));
  }

  #[test]
  fn collect_row_links_ignores_unbalanced_link_end() {
    // Arrange
    let target = LinkTarget::Internal(AnchorId::Label(LabelId::new("x")));
    let row = row(vec![cell(vec![
      HItem::LinkStart(target.clone()),
      HItem::LinkEnd,
      HItem::LinkEnd,
    ])]);
    let col_widths = vec![pt(30.0)];
    let columns = vec![TableColumn {
      align: ColumnAlign::Left,
      width: ColumnWidth::Auto,
    }];

    // Act
    let links = collect_row_links(&row, &columns, &col_widths, pt(0.0));

    // Assert
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x0, 0.0) && close(links[0].x1, 0.0));
  }

  #[test]
  fn collect_row_links_respects_column_alignment() {
    // Arrange
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(target),
      rule_box(10.0),
      HItem::LinkEnd,
    ])]);
    let col_widths = vec![pt(30.0)];
    let columns = vec![TableColumn {
      align: ColumnAlign::Right,
      width: ColumnWidth::Auto,
    }];

    // Act
    let links = collect_row_links(&row, &columns, &col_widths, pt(2.0));

    // Assert
    assert!(close(links[0].x0, 18.0) && close(links[0].x1, 28.0), "{links:?}");
  }
}
