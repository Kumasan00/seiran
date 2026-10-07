//! 根号（`MathRadical`）の配置 — 伸ばした根号記号・横線（vinculum）・被根号・指数
//!
//! `MathML Core` §3.3.3 の規則で、OpenType MATH の `Radical*` 定数と被根号のインク（グリフの形の範囲）から決める。
//! 根号記号 √ は被根号のインクの高さ + 深さ + ギャップ + 横線の太さ以上へ縦に伸ばし（区切り括弧と同じ伸縮）、インクの
//! 上端を横線の上端に合わせる。伸ばした記号が目標より高ければ、その余りの半分をギャップへ足して被根号を記号の縦の
//! 中ほどに置く（TeX の Rule 11。`MathML Core` はこの調整を持たず、size variant の段差ぶん記号が下へ垂れる）。横線は
//! 記号の送り幅の位置から被根号の幅いっぱいに引き、上に `RadicalExtraAscender` の余白を取る。指数は `MathML Core`
//! §3.3.3.3 どおり前後の kern で横に並べ、インクの底を「根号のインクの下端 + 根号の高さ × `RadicalDegreeBottomRaisePercent`」
//! に置く（根号の上端は横線の上端 + `RadicalExtraAscender`、下端は被根号と記号のインクの深い方）。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, PlacedHBox},
    boxing::{Measurer, Shaper, math},
    lowering::MathRadical,
  },
};

/// 根号記号（U+221A SQUARE ROOT。`MathML Core` の radical glyph）
const RADICAL_SIGN: &str = "\u{221A}";

/// 根号の配置に使う MATH 定数（根号の段のフォントサイズで長さへ換算し、display 段の値を選択済み）
#[derive(Debug, Clone, Copy)]
struct RadicalConstants {
  /// `RadicalVerticalGap`（display なら `RadicalDisplayStyleVerticalGap`）
  vertical_gap: Length,
  /// `RadicalRuleThickness`（フォント検証が 0 以上を保証する）
  rule_thickness: Length,
  /// `RadicalExtraAscender`
  extra_ascender: Length,
  /// `RadicalKernBeforeDegree`
  kern_before_degree: Length,
  /// `RadicalKernAfterDegree`
  kern_after_degree: Length,
  /// `RadicalDegreeBottomRaisePercent` を比にした値
  degree_bottom_raise: f64,
}

impl RadicalConstants {
  /// 数式フォントの MATH 定数を `font_size` で長さへ換算し、`display` なら display 段のギャップを選ぶ
  fn new(shaper: &Shaper<'_>, font_size: Length, display: bool) -> Self {
    let at = |constant: MathConstant| return shaper.math_constant(constant, font_size);
    return RadicalConstants {
      vertical_gap: at(if display {
        MathConstant::RadicalDisplayStyleVerticalGap
      } else {
        MathConstant::RadicalVerticalGap
      }),
      rule_thickness: at(MathConstant::RadicalRuleThickness),
      extra_ascender: at(MathConstant::RadicalExtraAscender),
      kern_before_degree: at(MathConstant::RadicalKernBeforeDegree),
      kern_after_degree: at(MathConstant::RadicalKernAfterDegree),
      degree_bottom_raise: shaper.math_ratio(MathConstant::RadicalDegreeBottomRaisePercent),
    };
  }

  /// 根号記号を縦に伸ばす目標の高さ（被根号のインクの高さ + 深さ + ギャップ + 横線の太さ）
  fn surd_target(&self, radicand_ink_height: Length, radicand_ink_depth: Length) -> Length {
    return radicand_ink_height + radicand_ink_depth + self.vertical_gap + self.rule_thickness;
  }

  /// 横線の下端の高さ（根号のベースライン基準）
  ///
  /// 被根号のインクの頂からギャップだけ上。伸ばした記号のインクの高さ `surd_ink_height` が目標を超えれば、その余りの
  /// 半分をギャップへ足す（目標に届かない記号でもギャップは縮めない）。
  fn vinculum_bottom(
    &self,
    radicand_ink_height: Length,
    radicand_ink_depth: Length,
    surd_ink_height: Length,
  ) -> Length {
    let excess = (surd_ink_height - self.surd_target(radicand_ink_height, radicand_ink_depth)).max(Length::ZERO);
    return radicand_ink_height + self.vertical_gap + excess / 2.0;
  }

  /// 指数の前と後の kern（前は 0 以上、後は −`degree_width` 以上へ切り詰める。`MathML Core` の
  /// `AdjustedRadicalKernBeforeDegree` / `AdjustedRadicalKernAfterDegree`）
  fn degree_kerns(&self, degree_width: Length) -> (Length, Length) {
    return (self.kern_before_degree.max(Length::ZERO), self.kern_after_degree.max(-degree_width));
  }

  /// 指数のベースラインを根号のベースラインから上げる量
  ///
  /// 根号の上端 `ascent`・下端の深さ `descent` に対し、指数のインクの底を 下端 + 高さ × 比 に置く。
  fn degree_shift(&self, ascent: Length, descent: Length, degree_ink_depth: Length) -> Length {
    return -descent + (ascent + descent).scale(self.degree_bottom_raise) + degree_ink_depth;
  }
}

impl Measurer<'_> {
  /// 根号を水平カーソル `dx`・縦オフセット `dy` に閉じた Atom 1 つとして置き、カーソルを根号の幅だけ進める
  ///
  /// Atom の高さは横線の上の `RadicalExtraAscender` を含む。
  pub(in crate::typeset::boxing) fn place_radical(
    &mut self,
    radical: MathRadical,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let MathRadical {
      degree,
      radicand,
      font_size,
      display,
    } = radical;
    let constants = RadicalConstants::new(&self.shaper, font_size, display);
    let radicand = self.detach(radicand);
    let target = constants.surd_target(radicand.ink_height, radicand.ink_depth);
    let (surd, (surd_top, surd_bottom)) = self.shaper.shape_vertical_delimiter(RADICAL_SIGN, font_size, target);
    let vinculum_bottom = constants.vinculum_bottom(radicand.ink_height, radicand.ink_depth, surd_top - surd_bottom);
    let vinculum_top = vinculum_bottom + constants.rule_thickness;
    let surd_dy = vinculum_top - surd_top;
    let ascent = vinculum_top + constants.extra_ascender;
    let descent = radicand.ink_depth.max(-(surd_bottom + surd_dy));

    let mut children = Vec::new();
    let mut surd_x = Length::ZERO;
    if let Some(degree) = degree {
      let degree = self.detach(degree);
      let (before, after) = constants.degree_kerns(degree.width);
      let shift = constants.degree_shift(ascent, descent, degree.ink_depth);
      surd_x = before + degree.width + after;
      math::translate_into(&mut children, degree.boxes, before, shift);
    }
    let body_x = surd_x + surd.width;
    children.push(PlacedHBox {
      hbox: surd,
      dx: surd_x,
      dy: surd_dy,
    });
    // 送り幅の和は負の送りを持つ字形で負になり得るので、横線の幅は 0 で止める（描画矩形の幅は非負）
    let radicand_width = radicand.width.max(Length::ZERO);
    children.push(PlacedHBox {
      hbox: HBox::rule(radicand_width, constants.rule_thickness),
      dx: body_x,
      dy: vinculum_bottom,
    });
    math::translate_into(&mut children, radicand.boxes, body_x, Length::ZERO);
    let mut atom = HBox::atom(children);
    atom.height = atom.height.max(ascent);
    atom.width = atom.width.max(body_x + radicand_width);
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

  /// STIX Two Math の text 段の値をフォント単位 1 = 1pt で写した定数（根号の段 1000pt 相当）
  fn constants() -> RadicalConstants {
    return RadicalConstants {
      vertical_gap: Length::pt(85.0),
      rule_thickness: Length::pt(68.0),
      extra_ascender: Length::pt(78.0),
      kern_before_degree: Length::pt(65.0),
      kern_after_degree: Length::pt(-335.0),
      degree_bottom_raise: 0.55,
    };
  }

  #[test]
  fn surd_target_covers_the_radicand_ink_the_gap_and_the_rule() {
    assert_eq!(constants().surd_target(Length::pt(479.0), Length::pt(10.0)), Length::pt(642.0));
  }

  #[test]
  fn vinculum_bottom_adds_half_of_the_surd_excess_to_the_gap() {
    let c = constants();

    assert_eq!(
      c.vinculum_bottom(Length::pt(479.0), Length::pt(10.0), Length::pt(1187.0)),
      Length::pt(836.5),
      "𝑥 の目標 642 に対し √ は 1187。余り 545 の半分をギャップ 85 へ足す"
    );
    assert_eq!(
      c.vinculum_bottom(Length::pt(479.0), Length::pt(10.0), Length::pt(642.0)),
      Length::pt(564.0),
      "ちょうど覆う記号ではギャップはそのまま"
    );
    assert_eq!(
      c.vinculum_bottom(Length::pt(479.0), Length::pt(10.0), Length::pt(500.0)),
      Length::pt(564.0),
      "覆いきれない記号（最大の size variant）でもギャップを縮めない"
    );
  }

  #[test]
  fn degree_kerns_clamp_before_to_zero_and_after_to_the_degree_width() {
    let c = constants();

    assert_eq!(c.degree_kerns(Length::pt(272.0)), (Length::pt(65.0), Length::pt(-272.0)), "狭い指数は −幅で止まる");
    assert_eq!(c.degree_kerns(Length::pt(816.0)), (Length::pt(65.0), Length::pt(-335.0)), "広い指数はフォントの値");
    let negative_before = RadicalConstants {
      kern_before_degree: Length::pt(-10.0),
      ..c
    };
    assert_eq!(negative_before.degree_kerns(Length::pt(272.0)).0, Length::ZERO, "前の kern は 0 以上");
  }

  #[test]
  fn degree_bottom_rises_by_the_percent_of_the_radical_height() {
    // 下端 −300 + (900 + 300) × 0.55 = 360 にインクの底を置くので、ベースラインは深さ 7 だけ上
    assert_eq!(constants().degree_shift(Length::pt(900.0), Length::pt(300.0), Length::pt(7.0)), Length::pt(367.0));
  }
}
