//! アクセント（`MathAccent`）の配置 — 基底の上にアクセント記号 1 字を重ねる
//!
//! 横位置は OpenType MATH の `MathTopAccentAttachment` で、アクセント記号の取付点を基底の取付点に揃える。基底の取付点は、
//! 数式フォントの 1 グリフならそのグリフの登録値（登録が無ければ送り幅の中央 — MATH の規定の既定値）、それ以外（複数
//! グリフ・スクリプト付き・分数等）は送り幅の中央（`MathML Core` §3.4.2.4 の取付点が無いときの既定）。アクセント記号の
//! 取付点は登録値で、登録が無ければ墨の横の中央（結合記号は送り幅 0 なので、送り幅の中央は原点＝墨の端になり字形の形と
//! 関係しない。登録のある結合記号の値も墨の中ほどにある）。
//! 縦位置は数式フォントの設計に従う。OpenType MATH の `AccentBaseHeight` は「アクセントを上げずに済む基底のインクの最大の
//! 高さ」で、アクセント字形はベースラインを基底に揃えるとインクの底がこの高さより上に来るように作られている（基底との
//! 隙間は字形自身が持つ）。そこで基底のインクの高さが `AccentBaseHeight` 以下ならベースラインを揃え、超えた分だけ
//! アクセントを上げ、隙間は足さない（TeX の Rule 12 の x-height を `AccentBaseHeight` に置き換えたもの）。`MathML Core`
//! §3.4.2.4 の本文は任意の要素を上付けにできる `MathML` 向けの一般化で、字形が隙間を持つことを前提にしないのでこれには
//! 従わない（同節のノートが、フォントの規則はベースラインを揃えるものだと述べる）。
//! 基底のインクの高さが `FlattenedAccentBaseHeight` を超えたら、アクセント記号を OpenType `flac` の平たい字形に替える
//! （上げる量の規則は変えない）。
//! Atom の幅は基底の送り幅で、アクセント記号の墨や原点のはみ出しは幅に数えない（TeX と同じ）。
//! 広幅アクセント（`wide`）は記号を基底の送り幅へ横にだけ伸ばす（元の字形 → 横方向の size variant → glyph assembly →
//! 最大の size variant。元の字形のままなら `ssty` / `flac` も上と同じ）。横位置・縦位置・幅の規則は変えない。伸ばした
//! size variant の取付点も上と同じ（登録値、無ければ墨の横の中央）。複数のパーツで組んだ glyph assembly は組み上がりの
//! 幅の中央（`MathML Core` の取付点の既定値）を取付点にし、パーツ 1 つで足りた assembly は 1 字形として size variant と
//! 同じに扱う。

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

/// 原点基準で配置した箱の列 `boxes` が数式フォントの 1 グリフだけなら、その箱の横位置・グリフの gid・フォントサイズ
fn sole_math_glyph(boxes: &[PlacedHBox]) -> Option<(Length, u32, Length)> {
  if let [placed] = boxes
    && let HBoxContent::Glyphs(run) = &placed.hbox.content
    && run.font_type == FontType::Math
    && let [glyph] = run.glyphs.as_slice()
  {
    return Some((placed.dx, glyph.gid, run.font_size));
  }
  return None;
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
      wide,
      font_size,
      script_level,
    } = accent;
    let base = self.detach(base);
    let base_attachment = match sole_math_glyph(&base.boxes) {
      Some((x, gid, size)) => {
        x + self.shaper.top_accent_attachment(gid, size).unwrap_or_else(|| {
          return self.shaper.math_glyph_advance(gid, size) / 2.0;
        })
      },
      None => base.width / 2.0,
    };
    let flattened = base.ink_height > self.shaper.math_constant(MathConstant::FlattenedAccentBaseHeight, font_size);
    let mark = if wide {
      self
        .shaper
        .shape_wide_accent(&accent, font_size, script_level, flattened, base.width.max(Length::ZERO))
    } else {
      self.shaper.shape_accent(&accent, font_size, script_level, flattened)
    };
    let mark_width = mark.width;
    let mark = vec![PlacedHBox {
      hbox: mark,
      dx: Length::ZERO,
      dy: Length::ZERO,
    }];
    let mark_attachment = match sole_math_glyph(&mark) {
      Some((x, gid, size)) => {
        x + self
          .shaper
          .top_accent_attachment(gid, size)
          .unwrap_or_else(|| return self.shaper.math_ink_center(gid, size))
      },
      // glyph assembly の広幅アクセント（送り幅の和は組み上がりの幅）と、数式フォントが結合記号を 1 グリフに組まないとき
      // で、組んだ列の送り幅の中央（MATH の取付点の既定値）に寄せる
      None => mark_width / 2.0,
    };
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
