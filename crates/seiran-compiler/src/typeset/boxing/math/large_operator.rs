//! display 段の大型演算子（`AtomNode::LargeOperator`）の配置 — 字形を display の大きさへ伸ばして数式軸に合わせる
//!
//! `MathML Core` §3.2.4.3（largeop を持ち math-style が normal の演算子）の規則で、演算子 1 字を区切り括弧と同じ伸縮
//! （size variant → glyph assembly → 最大の size variant）で `DisplayOperatorMinHeight` 以上へ縦に伸ばし、インクの縦中央を
//! 数式軸（`AxisHeight`）に合わせる（大型演算子は演算子辞書でどれも symmetric）。イタリック補正は選んだ字形のもので、
//! `Measurer::detach` が末尾のグリフとして、軸へ合わせたずれと一緒に拾う（math kern はそのグリフ自身のベースラインからの
//! 高さで引く）。glyph assembly で組んだときは `GlyphAssembly` の補正ではなく最後のパーツの補正になる。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{boxes::PlacedHBox, boxing::Measurer},
};

impl Measurer<'_> {
  /// display 段の大型演算子 `symbol`（フォントサイズ `font_size`）を水平カーソル `dx`・縦オフセット `dy` に置き、カーソルを
  /// 字形の送り幅だけ進める
  pub(in crate::typeset::boxing) fn place_large_operator(
    &mut self,
    symbol: &str,
    font_size: Length,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let target = self.shaper.math_constant(MathConstant::DisplayOperatorMinHeight, font_size);
    let (glyph, (top, bottom)) = self.shaper.shape_vertical_delimiter(symbol, font_size, target);
    let axis = self.shaper.math_constant(MathConstant::AxisHeight, font_size);
    let advance = glyph.width;
    out.push(PlacedHBox {
      hbox: glyph,
      dx: *dx,
      dy: dy + axis - (top + bottom) / 2.0,
    });
    *dx += advance;
  }
}
