//! 表ボックス（シェーピング済みの表全体）と表の純粋計測関数
//!
//! セル内容は計測済みの [`HItem`] 列として保持されるため、列幅の解決・行高の算出は
//! フォントに触れない純粋関数として本モジュールで提供する。罫線・行の描画は
//! `pdf_gen` 段で行う。

use crate::{ColumnWidth, HBoxContent, HItem, Length, TableColumn};

/// 表ボックス（シェーピング済みの表全体）
#[derive(Debug, Clone)]
pub struct TableBox {
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
pub struct TableRowBox {
  /// 行内のセル
  pub cells: Vec<TableCellBox>,
  /// この行の上に横罫線を引くか
  pub rule_above: bool,
}

/// 表の 1 セル（シェーピング済み）
#[derive(Debug, Clone)]
pub struct TableCellBox {
  /// セル内容のアイテム列（`Box` / `Kern` / `Glue` のみを想定）
  pub items: Vec<HItem>,
  /// 列方向の結合数（colspan、1 以上）
  pub span: u32,
}

/// アイテム列の自然幅を返す
///
/// box は計測済みの幅を持つため、フォントに触れずに合計できる。
#[must_use]
pub fn measure_items_width(items: &[HItem]) -> Length { return items.iter().map(HItem::natural_width).sum(); }

/// アイテム列に含まれるテキストの最大フォントサイズを返す（テキストがなければ `None`）
///
/// Atom（数式）の子要素も再帰的に走査する。
#[must_use]
pub fn max_font_size_in_items(items: &[HItem]) -> Option<Length> {
  return items
    .iter()
    .filter_map(|item| match item {
      HItem::Box(hbox) | HItem::FlushRight(hbox) => max_font_size_in_content(&hbox.content),
      _ => None,
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
      .filter_map(|child| max_font_size_in_content(&child.item.content))
      .reduce(Length::max),
  };
}

/// 行の高さ = 行内の最大フォントサイズ × 行高係数
#[must_use]
pub fn table_row_height(row: &TableRowBox, default_font_size: Length, line_height_factor: f32) -> Length {
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| max_font_size_in_items(&cell.items))
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
pub fn resolve_column_widths(table: &TableBox, available: Length, padding: Length) -> Vec<Length> {
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
          #[allow(clippy::cast_precision_loss)]
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
    #[allow(clippy::cast_precision_loss)]
    let share = ((available - used) / flex_indices.len() as f32).max(Length::ZERO);
    for i in flex_indices {
      widths[i] = share.max(naturals[i]);
    }
  }
  return widths;
}

#[cfg(test)]
mod tests {
  use super::{
    TableBox, TableCellBox, TableRowBox, max_font_size_in_items, measure_items_width, resolve_column_widths,
    table_row_height,
  };
  use crate::{
    ColumnAlign, ColumnWidth, FontType, GlyphRun, HBox, HBoxContent, HItem, Length, PlacedHItem, TableColumn,
  };

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に（sp 丸め精度内で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 指定幅・指定フォントサイズの Glyphs ボックスを作るヘルパ
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

  /// 指定幅の Rule ボックス（テキストを含まない）を作るヘルパ
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

  /// `span=1` のセルを作るヘルパ
  fn cell(items: Vec<HItem>) -> TableCellBox { return TableCellBox { items, span: 1 }; }

  /// `rule_above = false` の行を作るヘルパ
  fn row(cells: Vec<TableCellBox>) -> TableRowBox {
    return TableRowBox {
      cells,
      rule_above: false,
    };
  }

  #[test]
  fn measure_items_width_is_additive() {
    // Arrange — box(10) + glue(5) + kern(3) = 18
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

    // Act / Assert — Penalty / ForcedBreak は幅 0
    assert!(close(measure_items_width(&items), 18.0));
    assert!(close(measure_items_width(&[HItem::Penalty { value: 0 }, HItem::ForcedBreak]), 0.0));
  }

  #[test]
  fn max_font_size_in_items_returns_largest_text_size() {
    // Arrange — フォント 10 と 14 が混在
    let items = vec![glyph_box(20.0, 10.0), rule_box(5.0), glyph_box(20.0, 14.0)];

    // Act / Assert
    assert_eq!(max_font_size_in_items(&items), Some(pt(14.0)));
  }

  #[test]
  fn max_font_size_in_items_none_without_text() {
    // Rule のみ（テキストなし）なら None
    assert_eq!(max_font_size_in_items(&[rule_box(5.0)]), None);
  }

  #[test]
  fn max_font_size_in_items_recurses_into_atom() {
    // Arrange — Atom（数式）の子要素内の Glyphs フォントサイズも拾う
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
    // Arrange — フォント 10 と 12 のセル、行高係数 1.5 → 12 * 1.5 = 18
    let row = row(vec![
      cell(vec![glyph_box(10.0, 10.0)]),
      cell(vec![glyph_box(10.0, 12.0)]),
    ]);

    // Act / Assert
    assert!(close(table_row_height(&row, pt(9.0), 1.5), 18.0));
  }

  #[test]
  fn table_row_height_falls_back_to_default_font() {
    // Arrange — テキストのない行は default_font_size を使う（9 * 2.0 = 18）
    let row = row(vec![cell(vec![rule_box(5.0)])]);

    // Act / Assert
    assert!(close(table_row_height(&row, pt(9.0), 2.0), 18.0));
  }

  #[test]
  fn resolve_column_widths_fixed_auto_ratio() {
    // Arrange — Fixed(40) / Auto(内容30+padding4=34) / Ratio(0.5*200=100)
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
    // Arrange — Auto(20) + Flex。available=100、padding=0 → Flex は残り 80
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
    // Arrange — Auto(90) で残りが少なくても、Flex は自然幅(40)を下回らない
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

    // Act — 残り = 100 - 90 = 10 だが natural[1]=40
    let widths = resolve_column_widths(&table, pt(100.0), pt(0.0));

    // Assert
    assert!(close(widths[1], 40.0), "Flex は自然幅を下回らない: {widths:?}");
  }
}
