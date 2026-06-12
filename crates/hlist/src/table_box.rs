//! 表ボックス（シェーピング済みの表全体）と表の純粋計測関数
//!
//! セル内容は計測済みの [`HItem`] 列として保持されるため、列幅の解決・行高の算出は
//! フォントに触れない純粋関数として本モジュールで提供する。罫線・行の描画は
//! `pdf_gen` 段で行う。

use types::{ColumnWidth, TableColumn};

use crate::hitem::{HBoxContent, HItem};

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

/// アイテム列の自然幅（pt）を返す
///
/// box は計測済みの幅を持つため、フォントに触れずに合計できる。
#[must_use]
pub fn measure_items_width(items: &[HItem]) -> f32 { return items.iter().map(HItem::natural_width).sum(); }

/// アイテム列に含まれるテキストの最大フォントサイズを返す（テキストがなければ `None`）
///
/// Atom（数式）の子要素も再帰的に走査する。
#[must_use]
pub fn max_font_size_in_items(items: &[HItem]) -> Option<f32> {
  return items
    .iter()
    .filter_map(|item| match item {
      HItem::Box(hbox) => max_font_size_in_content(&hbox.content),
      _ => None,
    })
    .reduce(f32::max);
}

/// ボックス内容に含まれるテキストの最大フォントサイズを返す
fn max_font_size_in_content(content: &HBoxContent) -> Option<f32> {
  return match content {
    HBoxContent::Glyphs(run) => Some(run.font_size),
    HBoxContent::Rule { .. } => None,
    HBoxContent::Atom(children) => {
      children.iter().filter_map(|child| max_font_size_in_content(&child.item.content)).reduce(f32::max)
    },
  };
}

/// 行の高さ（pt）= 行内の最大フォントサイズ × 行高係数
#[must_use]
pub fn table_row_height(row: &TableRowBox, default_font_size: f32, line_height_factor: f32) -> f32 {
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| max_font_size_in_items(&cell.items))
    .reduce(f32::max)
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
pub fn resolve_column_widths(table: &TableBox, available: f32, padding: f32) -> Vec<f32> {
  let column_count = table.columns.len();
  let mut naturals = vec![0.0f32; column_count];

  for row in table.head.iter().chain(table.rows.iter()) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let span = cell.span as usize;
      if span == 1 && column_index < column_count {
        let width = measure_items_width(&cell.items) + 2.0 * padding;
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
        let width = measure_items_width(&cell.items) + 2.0 * padding;
        let current: f32 = naturals[column_index..column_index + span].iter().sum();
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

  let mut widths = vec![0.0f32; column_count];
  let mut flex_indices: Vec<usize> = Vec::new();
  let mut used = 0.0f32;
  for (i, column) in table.columns.iter().enumerate() {
    match column.width {
      ColumnWidth::Fixed(length) => {
        widths[i] = length.to_pt();
        used += widths[i];
      },
      ColumnWidth::Ratio(ratio) => {
        widths[i] = ratio * available;
        used += widths[i];
      },
      ColumnWidth::Auto => {
        widths[i] = naturals[i].max(2.0 * padding);
        used += widths[i];
      },
      ColumnWidth::Flex => flex_indices.push(i),
    }
  }
  if !flex_indices.is_empty() {
    #[allow(clippy::cast_precision_loss)]
    let share = ((available - used) / flex_indices.len() as f32).max(0.0);
    for i in flex_indices {
      widths[i] = share.max(naturals[i]);
    }
  }
  return widths;
}
