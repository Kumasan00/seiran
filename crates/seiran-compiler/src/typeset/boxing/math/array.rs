//! 数式に埋め込んだ行列・場合分け（`MathArray`）の配置 — セルのグリッドの縦中央を数式軸に載せ、括弧で挟む
//!
//! グリッドは表示数式ブロックと同じ規則で組み（列幅は列内のセル幅の最大値・行送りは前の行の深さ + 行間 + 次の行の
//! 高さ）、括弧の有無によらず縦中央を数式軸（`AxisHeight`）に載せる。括弧は表示数式ブロックと同じくグリッドの
//! 高さ + 深さ以上へ縦に伸ばしてインクの縦中央を数式軸に合わせ、本体との間に段のフォントサイズの 0.15 倍のアキを
//! 置く。全体は閉じた Atom 1 つ。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, PlacedHBox},
    boxing::{
      Measurer,
      math::{MeasuredCell, arrange::MathArray},
    },
  },
};

impl Measurer<'_> {
  /// 埋め込んだ行列・場合分けを水平カーソル `dx`・縦オフセット `dy` に閉じた Atom 1 つとして置き、カーソルを全体の
  /// 送り幅だけ進める
  pub(super) fn place_array(&mut self, array: MathArray, dy: Length, dx: &mut Length, out: &mut Vec<PlacedHBox>) {
    let MathArray {
      rows,
      align,
      delimiters,
      font_size,
      script_level,
      color,
      row_gap,
      column_gap,
    } = array;
    let cells = rows
      .into_iter()
      .map(|row| {
        return row
          .into_iter()
          .map(|content| {
            return MeasuredCell {
              content: self.build_math_atom(content),
              align,
            };
          })
          .collect();
      })
      .collect();
    let (grid, _) = Self::build_grid(cells, row_gap, column_gap);
    let axis = self.shaper.math_constant(MathConstant::AxisHeight, font_size);
    let grid_dy = axis - (grid.height - grid.depth) / 2.0;
    let hbox = if delimiters.is_present() {
      self.wrap_with_delimiters(grid, grid_dy, axis, delimiters, font_size, script_level, color)
    } else {
      HBox::atom(vec![PlacedHBox {
        hbox: grid,
        dx: Length::ZERO,
        dy: grid_dy,
      }])
    };
    let width = hbox.width;
    out.push(PlacedHBox { hbox, dx: *dx, dy });
    *dx += width;
  }
}
