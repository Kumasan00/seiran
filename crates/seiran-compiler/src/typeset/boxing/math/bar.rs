//! 上線・下線（`MathBar`）の配置 — 基底の上か下に、基底の送り幅いっぱいの罫線を引く
//!
//! OpenType MATH の `Overbar*` / `Underbar*` 定数に従う（TeX の Rule 9 / 10 のギャップ 3θ・太さ θ・余白 θ をフォントの値に
//! 置き換えたもの）。上線は基底のインクの頂から `OverbarVerticalGap` 上に罫線の下端を置き、太さは
//! `OverbarRuleThickness`、罫線の上に `OverbarExtraAscender` の余白を取る。下線は基底のインクの底から
//! `UnderbarVerticalGap` 下に罫線の上端を置き、太さは `UnderbarRuleThickness`、罫線の下に `UnderbarExtraDescender` の
//! 余白を取る。ギャップはインクで測り、余白は Atom の高さ・深さにだけ入れる（墨ではないので、外側の線・アクセント・
//! スクリプトはこれを基底の高さに数えない）。罫線の幅と Atom の幅は基底の送り幅で、全体は閉じた Atom 1 つ。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, PlacedHBox},
    boxing::{Measurer, math},
    lowering::MathBar,
  },
};

/// 罫線の下端の高さ（基底のベースライン基準・上が正）
///
/// 上線は基底のインクの高さ `ink_height` からギャップ `gap` だけ上、下線は基底のインクの深さ `ink_depth` からギャップと
/// 太さ `thickness` だけ下。
fn rule_bottom(over: bool, ink_height: Length, ink_depth: Length, gap: Length, thickness: Length) -> Length {
  if over {
    return ink_height + gap;
  }
  return -(ink_depth + gap + thickness);
}

impl Measurer<'_> {
  /// 上線・下線を水平カーソル `dx`・縦オフセット `dy` に閉じた Atom 1 つとして置き、カーソルを基底の送り幅だけ進める
  pub(in crate::typeset::boxing) fn place_bar(
    &mut self,
    bar: MathBar,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let MathBar {
      body,
      over,
      font_size,
    } = bar;
    let at = |constant: MathConstant| return self.shaper.math_constant(constant, font_size);
    // 太さはフォント検証が 0 以上を保証する
    let (gap, thickness, extra) = if over {
      (
        at(MathConstant::OverbarVerticalGap),
        at(MathConstant::OverbarRuleThickness),
        at(MathConstant::OverbarExtraAscender),
      )
    } else {
      (
        at(MathConstant::UnderbarVerticalGap),
        at(MathConstant::UnderbarRuleThickness),
        at(MathConstant::UnderbarExtraDescender),
      )
    };
    let body = self.detach(body);
    let bottom = rule_bottom(over, body.ink_height, body.ink_depth, gap, thickness);

    let mut children = Vec::new();
    math::translate_into(&mut children, body.boxes, Length::ZERO, Length::ZERO);
    // 送り幅の和は負の送りを持つ字形で負になり得るので、罫線の幅は 0 で止める（描画矩形の幅は非負）
    children.push(PlacedHBox {
      hbox: HBox::rule(body.width.max(Length::ZERO), thickness),
      dx: Length::ZERO,
      dy: bottom,
    });
    let mut atom = HBox::atom(children);
    if over {
      atom.height = atom.height.max(bottom + thickness + extra);
    } else {
      atom.depth = atom.depth.max(-bottom + extra);
    }
    atom.width = body.width;
    out.push(PlacedHBox {
      hbox: atom,
      dx: *dx,
      dy,
    });
    *dx += body.width;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rule_bottom_keeps_the_gap_from_the_base_ink() {
    let (gap, thickness) = (Length::pt(175.0), Length::pt(68.0));

    assert_eq!(
      rule_bottom(true, Length::pt(479.0), Length::pt(10.0), gap, thickness),
      Length::pt(654.0),
      "上線の下端はインクの頂からギャップ上"
    );
    assert_eq!(
      rule_bottom(false, Length::pt(479.0), Length::pt(220.0), gap, thickness),
      Length::pt(-463.0),
      "下線の上端はインクの底からギャップ下"
    );
    assert_eq!(rule_bottom(true, Length::ZERO, Length::ZERO, gap, thickness), gap, "空の基底はベースラインから");
  }
}
