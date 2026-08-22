//! krilla フォントの構築とグリフ変換。
//!
//! `Publication` が持つのはフォントのバイト列と構築設定だけなので、krilla の `Font` をここで組む
//! （#372 以前は compiler 側が構築済みの `Font` を渡していた）。

use std::{collections::HashMap, sync::Arc};

use krilla::text::{Font, GlyphId, KrillaGlyph, Tag};
use read_fonts::{FontRef, ReadError, TableProvider};
use seiran_compiler::{FontType, Glyph, PublicationFont, PublicationResources};

use crate::error::PdfRenderError;

/// フォント種別ごとの構築済み krilla フォント。
pub(crate) struct KrillaFonts {
  /// 全 19 フォント種別ぶんの krilla フォント
  fonts: HashMap<FontType, Font>,
}

impl KrillaFonts {
  /// 指定フォント種別の krilla フォントを返す。
  ///
  /// # Panics
  ///
  /// 構築経路は [`build_krilla_fonts`] 1 つで、そこが `FontType::ALL` を全件構築するため
  /// 欠落は起こらない（起きたら不変条件の破れなのでここで落とす）。
  pub(crate) fn font(&self, font_type: FontType) -> &Font {
    let Some(font) = self.fonts.get(&font_type) else {
      unreachable!("build_krilla_fonts が FontType::ALL を全件構築する: {font_type:?} が欠落している");
    };
    return font;
  }
}

/// 描画資源のフォントバイト列と構築設定から krilla フォント集合を構築する。
///
/// [`FontType::ALL`] の宣言順で構築する — `HashMap` の反復順は `RandomState` によりプロセスごとに
/// 変わるため、宣言順に固定しないと複数フォントが同時に不正な場合にどの [`PdfRenderError`] が
/// 返るかが実行のたびに変わってしまう（診断内容は実行のたびに同一という制約に反する）。
///
/// # Errors
///
/// フォントバイト列の解析に失敗した、またはフォントの生成に失敗した場合に [`PdfRenderError`] を返す。
pub(crate) fn build_krilla_fonts(resources: &PublicationResources) -> Result<KrillaFonts, PdfRenderError> {
  let mut fonts = HashMap::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    let publication_font = resources.font(font_type);
    let has_fvar = font_has_fvar(publication_font, font_type)?;
    fonts.insert(font_type, build_krilla_font(font_type, publication_font, has_fvar)?);
  }
  return Ok(KrillaFonts { fonts });
}

/// 指定フォントに `fvar`（バリアブルフォント軸）テーブルがあるかを判定する。
fn font_has_fvar(font: &PublicationFont, font_type: FontType) -> Result<bool, PdfRenderError> {
  let font_ref = FontRef::from_index(font.bytes.as_slice(), font.face.font_index)
    .map_err(|source| return PdfRenderError::FontParse { font_type, source })?;
  return match font_ref.fvar() {
    Ok(_) => Ok(true),
    Err(ReadError::TableIsMissing(_)) => Ok(false),
    Err(source) => Err(PdfRenderError::VariationTableRead { font_type, source }),
  };
}

/// 判定済みの `fvar` 有無に基づき krilla フォントを構築する。
///
/// バイト列は `Arc` を clone して krilla へ渡す（実バイト列は複製しない）。
///
/// # Panics
///
/// `fvar` を持つフォントに `variation_axes` が無い設定は、フォント資源の構築時に
/// `typeset::font::validate_font` が `typeset::font::validation::missing_variation_axes` として
/// 拒否しているため、ここまで届かない（届いたら不変条件の破れなので落とす）。
fn build_krilla_font(font_type: FontType, font: &PublicationFont, has_fvar: bool) -> Result<Font, PdfRenderError> {
  let data = Arc::clone(&font.bytes);
  if has_fvar {
    let Some(axes_config) = font.face.variation_axes.as_ref() else {
      unreachable!(
        "fvar を持つフォントの variation_axes 欠落は typeset::font::validate_font が拒否する: {font_type:?}"
      );
    };
    let axes = axes_config
      .iter()
      .map(|cfg_axis| {
        let tag = Tag::new(&cfg_axis.name);
        #[expect(
          clippy::cast_possible_truncation,
          reason = "krilla の variable font 軸値が f32 しか受け付けず、API 境界での精度低下は避けられない"
        )]
        let value = cfg_axis.value as f32;
        let axis = (tag, value);
        return axis;
      })
      .collect::<Vec<_>>();
    return Font::new_variable(data.into(), font.face.font_index, &axes)
      .ok_or(PdfRenderError::FontCreation { font_type });
  }
  return Font::new(data.into(), font.face.font_index).ok_or(PdfRenderError::FontCreation { font_type });
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
