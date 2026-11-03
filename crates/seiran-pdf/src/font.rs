//! krilla フォントの構築とグリフ変換。

use std::collections::HashMap;

use krilla::{
  Data,
  text::{Font, GlyphId, KrillaGlyph, Tag},
};
use seiran_compiler::{FontFace, Glyph, PublicationFont, PublicationResources};

use crate::error::PdfRenderError;

/// 実体（フォント種別 × 候補）ごとの構築済み krilla フォント。
pub(crate) struct KrillaFonts {
  /// 描画資源の全実体ぶんの krilla フォント
  fonts: HashMap<FontFace, Font>,
}

impl KrillaFonts {
  /// 指定実体の krilla フォントを返す。
  ///
  /// # Panics
  ///
  /// 構築経路は [`build_krilla_fonts`] 1 つで、そこが描画資源の全実体（`PublicationResources::faces`）を構築し、
  /// 描画するグリフ列の実体は同じ `Publication` の資源にあるため欠落は起こらない。
  pub(crate) fn font(&self, face: FontFace) -> &Font {
    let Some(font) = self.fonts.get(&face) else {
      unreachable!("build_krilla_fonts が描画資源の全実体を構築する: {face} が欠落している");
    };
    return font;
  }
}

/// 描画資源のフォントバイト列と構築設定から krilla フォント集合を構築する。
///
/// 実体の順（`PublicationResources::faces`）に構築するので、複数フォントが不正でも返る [`PdfRenderError`] は
/// 実行ごとに同じ。組版に使わなかった候補も構築する。
///
/// # Errors
///
/// krilla がフォントを生成できなかった場合に [`PdfRenderError`] を返す。
pub(crate) fn build_krilla_fonts(resources: &PublicationResources) -> Result<KrillaFonts, PdfRenderError> {
  let mut fonts = HashMap::new();
  for face in resources.faces() {
    fonts.insert(face, build_krilla_font(face, resources.font(face))?);
  }
  return Ok(KrillaFonts { fonts });
}

/// 軸の指定（無ければ空）で krilla フォントを構築する。
///
/// krilla の `Font::new` は空軸の `Font::new_variable` そのものなので、静的フォントもこの経路で組める。
/// 軸の指定と `fvar` の整合は `typeset::font::validation` が検証済み。
fn build_krilla_font(face: FontFace, font: &PublicationFont) -> Result<Font, PdfRenderError> {
  let axes = font
    .face
    .variation_axes
    .iter()
    .flatten()
    .map(|cfg_axis| {
      #[expect(
        clippy::cast_possible_truncation,
        reason = "krilla の variable font 軸値が f32 しか受け付けず、API 境界での精度低下は避けられない"
      )]
      let value = cfg_axis.value as f32;
      return (Tag::new(&cfg_axis.name), value);
    })
    .collect::<Vec<_>>();
  return Font::new_variable(Data::from(font.shared_bytes()), font.face.font_index, &axes)
    .ok_or(PdfRenderError::FontCreation { face });
}

/// レイアウト済みグリフ列を UPEM で正規化して Krilla のグリフ列へ変換する。
#[expect(
  clippy::cast_precision_loss,
  reason = "グリフ座標は font design unit の整数で、f32 の仮数部に収まる桁数しか持たない"
)]
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
