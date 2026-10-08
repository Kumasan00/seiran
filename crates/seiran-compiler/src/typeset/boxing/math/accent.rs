//! アクセント（`MathAccent`）の配置 — 基底の上にアクセント記号 1 字を重ねる
//!
//! 横位置は OpenType MATH の `MathTopAccentAttachment` で、アクセント記号の取付点を基底の取付点に揃える。取付点は、数式
//! フォントの 1 グリフならそのグリフの登録値（登録が無ければ送り幅の中央 — MATH の規定の既定値）、それ以外（複数グリフ・
//! スクリプト付き・分数等の基底）は送り幅の中央（`MathML Core` §3.4.2.4 の取付点が無いときの既定）。
//! 縦位置は数式フォントの設計に従う。OpenType MATH の `AccentBaseHeight` は「アクセントを上げずに済む基底のインクの最大の
//! 高さ」で、アクセント字形はベースラインを基底に揃えるとインクの底がこの高さより上に来るように作られている（基底との
//! 隙間は字形自身が持つ）。そこで基底のインクの高さが `AccentBaseHeight` 以下ならベースラインを揃え、超えた分だけ
//! アクセントを上げ、隙間は足さない（TeX の Rule 12 の x-height を `AccentBaseHeight` に置き換えたもの）。`MathML Core`
//! §3.4.2.4 の本文は任意の要素を上付けにできる `MathML` 向けの一般化で、字形が隙間を持つことを前提にしないのでこれには
//! 従わない（同節のノートが、フォントの規則はベースラインを揃えるものだと述べる）。
//! 基底のインクの高さが `FlattenedAccentBaseHeight` を超えたら、アクセント記号を OpenType `flac` の平たい字形に替える
//! （上げる量の規則は変えない）。
//! Atom の幅は基底の送り幅で、アクセント記号の墨や原点のはみ出しは幅に数えない（TeX と同じ）。

use read_fonts::tables::math::MathConstant;

use crate::{
  length::Length,
  project::FontType,
  typeset::{
    boxes::{HBox, HBoxContent, PlacedHBox},
    boxing::{Measurer, math},
    lowering::MathAccent,
  },
};

/// アクセントのベースラインを基底のベースラインから上げる量（基底のインクの高さが `accent_base_height` を超えた分。
/// 超えなければ 0）
fn accent_raise(base_ink_height: Length, accent_base_height: Length) -> Length {
  return (base_ink_height - accent_base_height).max(Length::ZERO);
}

impl Measurer<'_> {
  /// アクセントを水平カーソル `dx`・縦オフセット `dy` に閉じた Atom 1 つとして置き、カーソルを基底の送り幅だけ進める
  pub(in crate::typeset::boxing) fn place_accent(
    &mut self,
    accent: MathAccent,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    let MathAccent {
      base,
      accent,
      font_size,
      script_level,
    } = accent;
    let base = self.detach(base);
    let base_attachment = self.top_accent_attachment(&base.boxes, base.width);
    let flattened = base.ink_height > self.shaper.math_constant(MathConstant::FlattenedAccentBaseHeight, font_size);
    let mark = self.shaper.shape_accent(&accent, font_size, script_level, flattened);
    let mark_width = mark.width;
    let mark = vec![PlacedHBox {
      hbox: mark,
      dx: Length::ZERO,
      dy: Length::ZERO,
    }];
    let mark_attachment = self.top_accent_attachment(&mark, mark_width);
    let raise = accent_raise(base.ink_height, self.shaper.math_constant(MathConstant::AccentBaseHeight, font_size));

    let mut children = Vec::new();
    math::translate_into(&mut children, base.boxes, Length::ZERO, Length::ZERO);
    math::translate_into(&mut children, mark, base_attachment - mark_attachment, raise);
    let mut atom = HBox::atom(children);
    // 結合記号は送り幅 0 で、取付点によっては原点が基底の送り幅より右に来るので、幅は基底の送り幅に戻す
    atom.width = base.width;
    out.push(PlacedHBox {
      hbox: atom,
      dx: *dx,
      dy,
    });
    *dx += base.width;
  }

  /// 原点基準で配置した箱の列 `boxes`（送り幅 `width`）の上付けアクセントの取付点（原点からの横位置）
  ///
  /// 数式フォントの 1 グリフだけならそのグリフの `MathTopAccentAttachment`（登録が無ければ送り幅の中央）、それ以外は
  /// 送り幅の中央。
  fn top_accent_attachment(&self, boxes: &[PlacedHBox], width: Length) -> Length {
    if let [placed] = boxes
      && let HBoxContent::Glyphs(run) = &placed.hbox.content
      && run.font_type == FontType::Math
      && let [glyph] = run.glyphs.as_slice()
    {
      return placed.dx + self.shaper.top_accent_attachment(glyph.gid, run.font_size);
    }
    return width / 2.0;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn accent_rises_only_by_the_excess_over_accent_base_height() {
    // STIX Two Math の AccentBaseHeight 480 をフォント単位 1 = 1pt で写した値
    let accent_base_height = Length::pt(480.0);

    assert_eq!(accent_raise(Length::pt(479.0), accent_base_height), Length::ZERO, "𝑥 は揃える");
    assert_eq!(accent_raise(Length::pt(480.0), accent_base_height), Length::ZERO, "ちょうどの高さも揃える");
    assert_eq!(accent_raise(Length::pt(593.0), accent_base_height), Length::pt(113.0), "𝑡 は超えた分だけ上げる");
    assert_eq!(accent_raise(Length::ZERO, accent_base_height), Length::ZERO, "空の基底は揃える");
  }
}
