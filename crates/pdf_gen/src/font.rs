//! グリフを Krilla の型へ変換する。

use krilla::text::{GlyphId, KrillaGlyph};
use model::Glyph;

/// レイアウト済みグリフ列を UPEM で正規化して Krilla のグリフ列へ変換する。
#[allow(clippy::cast_precision_loss)]
pub(crate) fn convert_to_krilla_glyphs(glyphs: &[Glyph], upem: f32) -> Vec<KrillaGlyph> {
  let krilla_glyphs = glyphs
    .iter()
    .map(|glyph| {
      return KrillaGlyph::new(
        GlyphId::new(glyph.gid),
        glyph.x_advance as f32 / upem,
        glyph.x_offset as f32 / upem,
        glyph.y_offset as f32 / upem,
        glyph.y_advance as f32 / upem,
        glyph.range.clone(),
        None,
      );
    })
    .collect::<Vec<_>>();
  return krilla_glyphs;
}
