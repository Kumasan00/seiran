//! 上下に積む上付き・下付き（`MathScripts` の `limits` が真）の配置 — display 段の大型演算子の上限・下限
//!
//! `MathML Core` §3.4.2（`munderover` の基底が largeop を持つ埋め込み演算子の場合）の規則で、上限のベースラインを基底の
//! インクの頂から max(`UpperLimitBaselineRiseMin`, `UpperLimitGapMin` + 上限のインクの深さ) 上げ、下限のベースラインを基底の
//! インクの底から max(`LowerLimitBaselineDropMin`, `LowerLimitGapMin` + 下限のインクの高さ) 下げる（`OverExtraAscender` /
//! `UnderExtraDescender` は 0）。横は基底・上限・下限の送り幅の中央を揃え、上限を基底のイタリック補正
//! （`LargeOpItalicCorrection`）の半分だけ右へ、下限を半分だけ左へずらす。幅は 3 つの横の広がりの和集合で、後ろにアキ
//! （`SpaceAfterScript`）は置かない。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{
    boxes::PlacedHBox,
    boxing::{Measurer, Shaper, math},
    lowering::MathScripts,
  },
};

/// 上限・下限の配置に使う MATH 定数（基底の段のフォントサイズで長さへ換算済み。どれも下限値）
#[derive(Debug, Clone, Copy)]
struct LimitConstants {
  /// `UpperLimitGapMin`
  upper_gap: Length,
  /// `UpperLimitBaselineRiseMin`
  upper_baseline_rise: Length,
  /// `LowerLimitGapMin`
  lower_gap: Length,
  /// `LowerLimitBaselineDropMin`
  lower_baseline_drop: Length,
}

impl LimitConstants {
  /// 数式フォントの MATH 定数を `font_size` で長さへ換算する
  fn new(shaper: &Shaper<'_>, font_size: Length) -> Self {
    let at = |constant: MathConstant| return shaper.math_constant(constant, font_size);
    return LimitConstants {
      upper_gap: at(MathConstant::UpperLimitGapMin),
      upper_baseline_rise: at(MathConstant::UpperLimitBaselineRiseMin),
      lower_gap: at(MathConstant::LowerLimitGapMin),
      lower_baseline_drop: at(MathConstant::LowerLimitBaselineDropMin),
    };
  }

  /// 上限のベースラインを基底のベースラインから上げる量（基底のインクの頂 + `OverShift`）
  fn over_shift(&self, base_ink_height: Length, over_ink_depth: Length) -> Length {
    return base_ink_height + self.upper_baseline_rise.max(self.upper_gap + over_ink_depth);
  }

  /// 下限のベースラインを基底のベースラインから下げる量（基底のインクの底 + `UnderShift`）
  fn under_shift(&self, base_ink_depth: Length, under_ink_height: Length) -> Length {
    return base_ink_depth + self.lower_baseline_drop.max(self.lower_gap + under_ink_height);
  }
}

/// 基底・上限・下限（上限・下限は無ければ `None`）の送り幅から、各部品の左端の位置と全体の幅を
/// （基底, 上限, 下限, 幅）で返す
///
/// 3 つの中央を揃え、上限は `italic_correction` の半分だけ右へ、下限は半分だけ左へずらす。位置は 3 つの横の広がりの
/// 和集合の左端を 0 とし、上限・下限の位置は送り幅が `Some` のときだけ `Some`。
fn limit_columns(
  base_width: Length,
  over_width: Option<Length>,
  under_width: Option<Length>,
  italic_correction: Length,
) -> (Length, Option<Length>, Option<Length>, Length) {
  let half_correction = italic_correction / 2.0f64;
  // 基底の中央を 0 とした、各部品の左端と送り幅
  let base = (-(base_width / 2.0f64), base_width);
  let over = over_width.map(|width| return (half_correction - width / 2.0f64, width));
  let under = under_width.map(|width| return (-half_correction - width / 2.0f64, width));
  let parts = [Some(base), over, under];
  let left = parts.iter().flatten().map(|&(left, _)| return left).fold(base.0, Length::min);
  let right = parts.iter().flatten().map(|&(left, width)| return left + width).fold(base.0 + base.1, Length::max);
  let column = |(part_left, _): (Length, Length)| return part_left - left;
  return (column(base), over.map(column), under.map(column), right - left);
}

impl Measurer<'_> {
  /// 上下に積む上付き・下付き（`scripts.limits` が真）を水平カーソル `dx`・縦オフセット `dy` から絶対配置し、カーソルを
  /// 3 つの広がりの和集合の幅だけ進める
  pub(in crate::typeset::boxing) fn place_limits(
    &mut self,
    scripts: MathScripts,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let MathScripts {
      base,
      superscript,
      subscript,
      font_size,
      // 上下に積むときは上付きのシフトを使わないので、cramped は配置に効かない
      cramped: _,
      // place_atom_node が limits の真のものだけをここへ振り分ける
      limits: _,
    } = scripts;
    let constants = LimitConstants::new(&self.shaper, font_size);
    let base = self.detach(base);
    let over = superscript.map(|nodes| return self.detach(nodes));
    let under = subscript.map(|nodes| return self.detach(nodes));
    let correction = base
      .trailing_glyph
      .map_or(Length::ZERO, |(gid, size, _)| return self.shaper.italic_correction(gid, size));
    let (base_x, over_x, under_x, width) = limit_columns(
      base.width,
      over.as_ref().map(|part| return part.width),
      under.as_ref().map(|part| return part.width),
      correction,
    );

    math::translate_into(out, base.boxes, *dx + base_x, dy);
    // 上限・下限の位置は送り幅を渡した側だけ Some なので、zip は部品の有無と一致する
    if let Some((over, over_x)) = over.zip(over_x) {
      let shift = constants.over_shift(base.ink_height, over.ink_depth);
      math::translate_into(out, over.boxes, *dx + over_x, dy + shift);
    }
    if let Some((under, under_x)) = under.zip(under_x) {
      let shift = constants.under_shift(base.ink_depth, under.ink_height);
      math::translate_into(out, under.boxes, *dx + under_x, dy - shift);
    }
    *dx += width;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// STIX Two Math の値をフォント単位 1 = 1pt で写した定数（演算子の段 1000pt 相当）
  fn constants() -> LimitConstants {
    return LimitConstants {
      upper_gap: Length::pt(135.0),
      upper_baseline_rise: Length::pt(300.0),
      lower_gap: Length::pt(135.0),
      lower_baseline_drop: Length::pt(670.0),
    };
  }

  #[test]
  fn over_shift_takes_the_larger_of_the_rise_and_the_gap() {
    let c = constants();

    assert_eq!(c.over_shift(Length::pt(782.0), Length::ZERO), Length::pt(1082.0), "浅い上限は 頂 + 上昇の下限 300");
    assert_eq!(
      c.over_shift(Length::pt(782.0), Length::pt(200.0)),
      Length::pt(1117.0),
      "深い上限は 頂 + ギャップ 135 + 深さ 200"
    );
  }

  #[test]
  fn under_shift_takes_the_larger_of_the_drop_and_the_gap() {
    let c = constants();

    assert_eq!(
      c.under_shift(Length::pt(248.0), Length::pt(479.0)),
      Length::pt(918.0),
      "低い下限は 底 + 降下の下限 670"
    );
    assert_eq!(
      c.under_shift(Length::pt(248.0), Length::pt(600.0)),
      Length::pt(983.0),
      "背の高い下限は 底 + ギャップ 135 + 高さ 600"
    );
  }

  #[test]
  fn limit_columns_center_all_parts_without_italic_correction() {
    let columns = limit_columns(Length::pt(1000.0), Some(Length::pt(500.0)), Some(Length::pt(1500.0)), Length::ZERO);

    assert_eq!(
      columns,
      (Length::pt(250.0), Some(Length::pt(500.0)), Some(Length::ZERO), Length::pt(1500.0)),
      "最も広い下限が幅を決め、基底と上限はその中央"
    );
  }

  #[test]
  fn limit_columns_shift_limits_by_half_the_italic_correction() {
    // 中央は 基底 0・上限 +100・下限 −100。左端 −600・右端 +600
    let columns =
      limit_columns(Length::pt(1000.0), Some(Length::pt(1000.0)), Some(Length::pt(1000.0)), Length::pt(200.0));

    assert_eq!(
      columns,
      (Length::pt(100.0), Some(Length::pt(200.0)), Some(Length::ZERO), Length::pt(1200.0)),
      "上限は補正の半分だけ右、下限は半分だけ左"
    );
  }

  #[test]
  fn limit_columns_skip_a_missing_limit() {
    let columns = limit_columns(Length::pt(1000.0), None, Some(Length::pt(400.0)), Length::ZERO);

    assert_eq!(columns, (Length::ZERO, None, Some(Length::pt(300.0)), Length::pt(1000.0)));
  }
}
