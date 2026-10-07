//! 分数（`MathFraction`）の配置 — 分子・分母を数式軸上の横罫の上下に積む
//!
//! `MathML Core` §3.3.2.1（分数線の太さが 0 でない分数）の規則で、シフト量を OpenType MATH の `Fraction*` 定数と
//! 分子・分母のインク（グリフの形の範囲）から決める。分子・分母は両者の送り幅の最大の中で中央に置き、横罫はその幅
//! いっぱいに中心を数式軸（`AxisHeight`）に合わせる。左右には `MathML Core` の UA スタイルシート
//! （`mfrac { padding-inline: 1px }`）のアキを置き、隣り合う分数の罫がつながらないようにする。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{
    boxes::{Align, HBox, PlacedHBox},
    boxing::{Measurer, Shaper, math},
    lowering::MathFraction,
  },
};

/// 分数の左右それぞれに空けるアキ（`MathML Core` の UA スタイルシートの 1px。CSS の 1px は 0.75pt = 49152sp）
const FRACTION_PADDING: Length = Length::from_sp(49_152);

/// 分数の配置に使う MATH 定数（分数の段のフォントサイズで長さへ換算し、display 段の値を選択済み）
#[derive(Debug, Clone, Copy)]
struct FractionConstants {
  /// `AxisHeight`
  axis_height: Length,
  /// `FractionRuleThickness`（フォント検証が 0 以上を保証する）
  rule_thickness: Length,
  /// `FractionNumeratorShiftUp`（display なら `FractionNumeratorDisplayStyleShiftUp`）
  numerator_shift_up: Length,
  /// `FractionDenominatorShiftDown`（display なら `FractionDenominatorDisplayStyleShiftDown`）
  denominator_shift_down: Length,
  /// `FractionNumeratorGapMin`（display なら `FractionNumDisplayStyleGapMin`）
  numerator_gap_min: Length,
  /// `FractionDenominatorGapMin`（display なら `FractionDenomDisplayStyleGapMin`）
  denominator_gap_min: Length,
}

impl FractionConstants {
  /// 数式フォントの MATH 定数を `font_size` で長さへ換算し、`display` なら display 段の値を選ぶ
  fn new(shaper: &Shaper<'_>, font_size: Length, display: bool) -> Self {
    let at = |constant: MathConstant| return shaper.math_constant(constant, font_size);
    let pick = |display_constant: MathConstant, constant: MathConstant| {
      return at(if display { display_constant } else { constant });
    };
    return FractionConstants {
      axis_height: at(MathConstant::AxisHeight),
      rule_thickness: at(MathConstant::FractionRuleThickness),
      numerator_shift_up: pick(
        MathConstant::FractionNumeratorDisplayStyleShiftUp,
        MathConstant::FractionNumeratorShiftUp,
      ),
      denominator_shift_down: pick(
        MathConstant::FractionDenominatorDisplayStyleShiftDown,
        MathConstant::FractionDenominatorShiftDown,
      ),
      numerator_gap_min: pick(MathConstant::FractionNumDisplayStyleGapMin, MathConstant::FractionNumeratorGapMin),
      denominator_gap_min: pick(MathConstant::FractionDenomDisplayStyleGapMin, MathConstant::FractionDenominatorGapMin),
    };
  }

  /// 分子のベースラインを分数のベースラインから上げる量
  ///
  /// 標準のシフトと、分子のインクの底を横罫の上端からギャップの下限だけ離す量の大きい方。
  fn numerator_shift(&self, numerator_ink_depth: Length) -> Length {
    return self
      .numerator_shift_up
      .max(self.axis_height + self.rule_thickness / 2.0 + self.numerator_gap_min + numerator_ink_depth);
  }

  /// 分母のベースラインを分数のベースラインから下げる量
  ///
  /// 標準のシフトと、分母のインクの頂を横罫の下端からギャップの下限だけ離す量の大きい方。
  fn denominator_shift(&self, denominator_ink_height: Length) -> Length {
    return self
      .denominator_shift_down
      .max(self.rule_thickness / 2.0 + self.denominator_gap_min + denominator_ink_height - self.axis_height);
  }
}

impl Measurer<'_> {
  /// 分数を水平カーソル `dx`・縦オフセット `dy` に閉じた Atom 1 つとして置き、カーソルを分数の幅だけ進める
  ///
  /// 分子・分母・横罫は Atom の左端から [`FRACTION_PADDING`] の位置に置き、Atom の幅は両側のアキを含む。
  pub(in crate::typeset::boxing) fn place_fraction(
    &mut self,
    fraction: MathFraction,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let MathFraction {
      numerator,
      denominator,
      font_size,
      display,
    } = fraction;
    let constants = FractionConstants::new(&self.shaper, font_size, display);
    let numerator = self.detach(numerator);
    let denominator = self.detach(denominator);
    // 送り幅の和は負の送りを持つ字形で負になり得るので、罫の幅は 0 で止める（描画矩形の幅は非負）
    let width = numerator.width.max(denominator.width).max(Length::ZERO);

    let mut children = Vec::new();
    math::translate_into(
      &mut children,
      numerator.boxes,
      FRACTION_PADDING + Align::Center.offset(width, numerator.width),
      constants.numerator_shift(numerator.ink_depth),
    );
    math::translate_into(
      &mut children,
      denominator.boxes,
      FRACTION_PADDING + Align::Center.offset(width, denominator.width),
      -constants.denominator_shift(denominator.ink_height),
    );
    children.push(PlacedHBox {
      hbox: HBox::rule(width, constants.rule_thickness),
      dx: FRACTION_PADDING,
      dy: constants.axis_height - constants.rule_thickness / 2.0,
    });
    let mut atom = HBox::atom(children);
    atom.width = width + FRACTION_PADDING + FRACTION_PADDING;
    let advance = atom.width;
    out.push(PlacedHBox {
      hbox: atom,
      dx: *dx,
      dy,
    });
    *dx += advance;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// STIX Two Math の text 段の値をフォント単位 1 = 1pt で写した定数（分数の段 1000pt 相当）
  fn text_constants() -> FractionConstants {
    return FractionConstants {
      axis_height: Length::pt(258.0),
      rule_thickness: Length::pt(68.0),
      numerator_shift_up: Length::pt(585.0),
      denominator_shift_down: Length::pt(585.0),
      numerator_gap_min: Length::pt(68.0),
      denominator_gap_min: Length::pt(68.0),
    };
  }

  /// STIX Two Math の display 段の値（`*DisplayStyle*`）
  fn display_constants() -> FractionConstants {
    return FractionConstants {
      numerator_shift_up: Length::pt(640.0),
      denominator_shift_down: Length::pt(640.0),
      numerator_gap_min: Length::pt(150.0),
      denominator_gap_min: Length::pt(150.0),
      ..text_constants()
    };
  }

  #[test]
  fn numerator_shift_takes_the_larger_of_the_standard_shift_and_the_gap() {
    let c = text_constants();

    assert_eq!(c.numerator_shift(Length::pt(12.0)), Length::pt(585.0), "浅い分子は標準のシフト");
    assert_eq!(
      c.numerator_shift(Length::pt(300.0)),
      Length::pt(660.0),
      "深い分子は 軸 258 + 罫の半分 34 + ギャップ 68 + 深さ 300"
    );
    assert_eq!(
      display_constants().numerator_shift(Length::pt(220.0)),
      Length::pt(662.0),
      "display のギャップ 150 では 𝑦 の深さ 220 で標準の 640 を上回る"
    );
  }

  #[test]
  fn denominator_shift_takes_the_larger_of_the_standard_shift_and_the_gap() {
    let c = text_constants();

    assert_eq!(
      c.denominator_shift(Length::pt(705.0)),
      Length::pt(585.0),
      "34 + 68 + 705 − 258 = 549 は標準に届かない"
    );
    assert_eq!(
      c.denominator_shift(Length::pt(900.0)),
      Length::pt(744.0),
      "背の高い分母は 罫の半分 + ギャップ + 高さ − 軸"
    );
  }
}
