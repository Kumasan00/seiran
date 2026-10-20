//! 伸縮括弧（`MathFenced`）の配置 — 中身を左右の区切り括弧で挟み、括弧を中身の高さへ縦に伸ばす
//!
//! `MathML Core` の対称な伸縮（区切り括弧は演算子辞書でどれも symmetric）で、括弧を中身のインクの高さ h・深さ d と
//! 数式軸 a（`AxisHeight`）から `2 × max(h − a, d + a)` 以上へ、cases / matrix の枠・根号と同じ伸縮（size variant →
//! glyph assembly）で縦にだけ伸ばし、インクの縦中央を数式軸に合わせる。中身が軸の上下に非対称でも、括弧は軸を中心に
//! 上下対称に伸びて両側を覆う。括弧と中身の間にアキは足さない（TeX の `\left` / `\right` と同じ）。中間の区切り（区画の間の
//! 縦棒 [`FENCE_DIVIDER`]）も同じ目標・同じ軸合わせで伸ばし、目標には寄与しない。全体は閉じた Atom 1 つ。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, PlacedHBox},
    boxing::{Measurer, math},
    lowering::MathFenced,
  },
};

/// 伸縮括弧の中間の区切り `\given` の字（U+007C。`\abs` の括弧と同じ字）
pub(in crate::typeset) const FENCE_DIVIDER: &str = "|";

/// 中身のインクの高さ `ink_height`・深さ `ink_depth` を、数式軸の高さ `axis` を中心に上下対称に覆う括弧の高さ + 深さ
fn symmetric_target(ink_height: Length, ink_depth: Length, axis: Length) -> Length {
  return (ink_height - axis).max(ink_depth + axis) * 2.0;
}

impl Measurer<'_> {
  /// 伸縮括弧を水平カーソル `dx`・縦オフセット `dy` に閉じた Atom 1 つとして置き、カーソルを全体の送り幅だけ進める
  pub(in crate::typeset::boxing) fn place_fenced(
    &mut self,
    fenced: MathFenced,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let MathFenced {
      segments,
      open,
      close,
      font_size,
      script_level,
      color,
    } = fenced;
    let segments: Vec<_> = segments.into_iter().map(|segment| return self.detach(segment)).collect();
    // 区画のインクは 0 以上なので、区画ごとの最大がそのまま中身全体のインクの高さ・深さになる
    let ink_height = segments.iter().map(|segment| return segment.ink_height).fold(Length::ZERO, Length::max);
    let ink_depth = segments.iter().map(|segment| return segment.ink_depth).fold(Length::ZERO, Length::max);
    let axis = self.shaper.math_constant(MathConstant::AxisHeight, font_size);
    let target = symmetric_target(ink_height, ink_depth, axis);
    let mut shape =
      |text: &str| return self.shaper.shape_vertical_delimiter(text, font_size, script_level, target, color);
    let (open, open_ink) = shape(&open);
    let (close, close_ink) = shape(&close);
    let dividers: Vec<_> = segments.iter().skip(1).map(|_| return shape(FENCE_DIVIDER)).collect();
    let on_axis = |(top, bottom): (Length, Length)| return axis - (top + bottom) / 2.0;

    let mut x = open.width;
    let mut children = vec![PlacedHBox {
      hbox: open,
      dx: Length::ZERO,
      dy: on_axis(open_ink),
    }];
    let mut dividers = dividers.into_iter();
    for (index, segment) in segments.into_iter().enumerate() {
      if index > 0 {
        let (divider, divider_ink) = dividers.next().expect("縦棒は 2 番目以降の区画の数だけ組んだ");
        let divider_width = divider.width;
        children.push(PlacedHBox {
          hbox: divider,
          dx: x,
          dy: on_axis(divider_ink),
        });
        x += divider_width;
      }
      math::translate_into(&mut children, segment.boxes, x, Length::ZERO);
      x += segment.width;
    }
    let width = x + close.width;
    children.push(PlacedHBox {
      hbox: close,
      dx: x,
      dy: on_axis(close_ink),
    });
    out.push(PlacedHBox {
      hbox: HBox::atom(children),
      dx: *dx,
      dy,
    });
    *dx += width;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn symmetric_target_covers_the_farther_side_of_the_axis() {
    let axis = Length::pt(250.0);

    assert_eq!(symmetric_target(Length::pt(900.0), Length::pt(100.0), axis), Length::pt(1300.0), "軸より上が遠い");
    assert_eq!(symmetric_target(Length::pt(300.0), Length::pt(600.0), axis), Length::pt(1700.0), "軸より下が遠い");
    assert_eq!(symmetric_target(Length::ZERO, Length::ZERO, axis), Length::pt(500.0), "空の中身は軸の 2 倍");
  }
}
