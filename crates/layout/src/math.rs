//! ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::MathBlock`）
//!
//! 各セル（`&` 区切りの列）を閉じた Atom（`HBox`）に measure し、環境種別 [`MathEnvKind`]
//! に応じて列を整列・行を縦積みして「環境全体 = 1 つの本体 Atom」に合成する。数式の列は
//! すべて自然幅なので、列幅・行高・区切り括弧の配置はすべてこの段で局所座標まで確定できる。
//! 本文幅に依存する処理（本体の中央寄せ・番号の端寄せ）だけを `hlist::break_pages` に委ねる。

use hlist::{Block, HBox, MathRowNumber, PlacedHItem};
use lowering::MathBlockRow;
use types::{Align, MathEnvKind};

use crate::Measurer;

/// セルの列内での水平揃え（環境種別ごとに決まる）
#[derive(Debug, Clone, Copy)]
enum CellAlign {
  /// 左揃え
  Left,
  /// 中央揃え
  Center,
  /// 右揃え
  Right,
}

/// 環境種別と列インデックスから、その列のセルの水平揃えを決める
///
/// - `align`: 奇数列（0 始まりで偶数番目）を右、続く列を左に寄せて `&` 位置で接合する
/// - `matrix`: 全セル中央揃え
/// - `equation` / `gather` / `cases`: 左揃え（`gather` の行中央寄せは将来対応）
fn column_align(kind: MathEnvKind, col: usize) -> CellAlign {
  return match kind {
    MathEnvKind::Align => {
      if col.is_multiple_of(2) {
        CellAlign::Right
      } else {
        CellAlign::Left
      }
    },
    MathEnvKind::Matrix { .. } => CellAlign::Center,
    MathEnvKind::Equation | MathEnvKind::Gather | MathEnvKind::Cases => CellAlign::Left,
  };
}

/// 列幅 `col_width` の中に幅 `cell_width` のセルを置くときの水平オフセット（pt）
fn column_offset(align: CellAlign, col_width: f32, cell_width: f32) -> f32 {
  return match align {
    CellAlign::Left => 0.0,
    CellAlign::Center => (col_width - cell_width) / 2.0,
    CellAlign::Right => col_width - cell_width,
  };
}

/// 行を measure したあとの中間表現
struct MeasuredRow {
  /// セルごとの閉じた Atom
  cells: Vec<HBox>,
  /// 行番号ボックス（採番された行のみ）
  number: Option<HBox>,
}

impl Measurer<'_> {
  /// `LayoutNode::MathBlock` を measure して `Block::MathBlock` に合成する
  ///
  /// 各セルを `build_atom` で閉じた Atom にし、列ごとの最大幅で列位置を確定、行を縦に
  /// 積んで 1 つの本体 Atom にまとめる。番号は行ごとに別ボックスとして保持し、最終的な
  /// 端寄せは `break_pages` に委ねる。
  pub(crate) fn build_math_block(
    &mut self,
    kind: MathEnvKind,
    rows: Vec<MathBlockRow>,
    align: Align,
    numbers_on_right: bool,
    row_gap: f32,
    column_gap: f32,
  ) -> Block {
    // 各セル・番号を閉じた Atom に measure する
    let measured: Vec<MeasuredRow> = rows
      .into_iter()
      .map(|row| {
        let cells = row.cells.into_iter().map(|cell| self.build_atom(0.0, cell)).collect();
        let number = row.number.map(|number| self.build_atom(0.0, number));
        return MeasuredRow { cells, number };
      })
      .collect();

    // 列幅 = その列のセルの最大幅。列位置 = 列幅 + 列間アキの累積
    let ncols = measured.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    let mut col_widths = vec![0.0f32; ncols];
    for row in &measured {
      for (c, cell) in row.cells.iter().enumerate() {
        col_widths[c] = col_widths[c].max(cell.width);
      }
    }
    let mut col_x = vec![0.0f32; ncols];
    let mut acc = 0.0f32;
    for c in 0..ncols {
      col_x[c] = acc;
      acc += col_widths[c] + column_gap;
    }

    // 行を縦に積む。行 0 のベースラインを本体ベースライン（dy = 0）とし、以降は下方向へ。
    let mut placed: Vec<PlacedHItem> = Vec::new();
    let mut numbers: Vec<MathRowNumber> = Vec::new();
    let mut baseline_dy = 0.0f32;
    let mut prev_depth = 0.0f32;
    for (i, row) in measured.into_iter().enumerate() {
      let row_height = row.cells.iter().map(|cell| cell.height).fold(0.0f32, f32::max);
      let row_depth = row.cells.iter().map(|cell| cell.depth).fold(0.0f32, f32::max);
      if i > 0 {
        baseline_dy -= prev_depth + row_gap + row_height;
      }
      for (c, cell) in row.cells.into_iter().enumerate() {
        let intra = column_offset(column_align(kind, c), col_widths[c], cell.width);
        placed.push(PlacedHItem {
          item: cell,
          dy: baseline_dy,
          dx: col_x[c] + intra,
        });
      }
      if let Some(number) = row.number {
        numbers.push(MathRowNumber {
          content: number,
          dy: baseline_dy,
        });
      }
      prev_depth = row_depth;
    }

    return Block::MathBlock {
      body: HBox::atom(placed),
      numbers,
      numbers_on_right,
      align,
    };
  }
}
