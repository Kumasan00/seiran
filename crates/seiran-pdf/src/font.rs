//! krilla フォントの構築とグリフ変換。
//!
//! `Publication` が持つのはフォントのバイト列と構築設定だけなので、krilla の `Font` をここで組む
//! （#372 以前は compiler 側が構築済みの `Font` を渡していた）。

use std::{collections::HashMap, sync::Arc};

use krilla::{
  Data,
  text::{Font, GlyphId, KrillaGlyph, Tag},
};
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
/// krilla がフォントを生成できなかった場合に [`PdfRenderError`] を返す。
pub(crate) fn build_krilla_fonts(resources: &PublicationResources) -> Result<KrillaFonts, PdfRenderError> {
  let mut fonts = HashMap::with_capacity(FontType::ALL.len());
  for &font_type in FontType::ALL {
    fonts.insert(font_type, build_krilla_font(font_type, resources.font(font_type))?);
  }
  return Ok(KrillaFonts { fonts });
}

/// krilla へフォントバイト列を渡すための `AsRef<[u8]>` 包み。
///
/// krilla の [`Data`] を**バイト列を複製せず**作れる経路は `Arc<Vec<u8>>` と
/// `Arc<dyn AsRef<[u8]> + Send + Sync>` の 2 つだけで、`Publication` が持つ `Arc<[u8]>` は
/// どちらにも直接は当たらない（スライスは `Sized` でないので `Arc<[u8]>` は
/// `Arc<dyn AsRef<[u8]>>` へ unsize できない）。包むのは共有ハンドルだけで、バイト列は複製しない。
struct FontBytes(Arc<[u8]>);

impl AsRef<[u8]> for FontBytes {
  /// 包んだ共有ハンドルの中身を借用する。
  fn as_ref(&self) -> &[u8] { return &self.0; }
}

/// 共有ハンドルを複製して krilla の [`Data`] を作る（実バイト列は複製しない）。
fn krilla_data(bytes: &Arc<[u8]>) -> Data {
  let shared: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(FontBytes(Arc::clone(bytes)));
  return Data::from(shared);
}

/// 軸の指定（無ければ空）で krilla フォントを構築する。
///
/// krilla の `Font::new` は空軸の `Font::new_variable` そのものなので、静的 / 可変で呼び分けない。
/// 軸の指定と `fvar` の整合（あるのに指定が無い・無いのに指定がある・読めない）は
/// `typeset::font::validation` が検証済みで、renderer はフォントを自分でパースしない（#681）。
/// バイト列は [`krilla_data`] で共有ハンドルのまま渡す（実バイト列は複製しない）。
fn build_krilla_font(font_type: FontType, font: &PublicationFont) -> Result<Font, PdfRenderError> {
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
  return Font::new_variable(krilla_data(&font.bytes), font.face.font_index, &axes)
    .ok_or(PdfRenderError::FontCreation { font_type });
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
