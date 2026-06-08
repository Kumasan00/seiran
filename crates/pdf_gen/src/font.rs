//! Krilla フォント / グリフへのアダプタ。
//!
//! `config` と解析済みフォント参照から Krilla の [`Font`] 集合を組み立てる
//! [`build_krilla_fonts`] と、レイアウト済みの [`Glyph`] 列を UPEM 正規化した
//! [`KrillaGlyph`] 列へ変換する [`convert_to_krilla_glyphs`] を提供する。

use font::{FontData, FontRefs};
use krilla::text::{Font, GlyphId, KrillaGlyph, Tag};
use layout::Glyph;
use read_config::Config;
use read_fonts::{ReadError, TableProvider};
use types::{FontMap, FontType};

use crate::error::PdfGenError;

/// 設定に基づいて Krilla 用フォント集合を構築します。
///
/// バリアブルフォントの場合は `variation_axes` を必須とし、
/// 通常フォントの場合は `font_index` を使って `Font` を生成します。
pub(crate) fn build_krilla_fonts(
  config: &Config,
  font_bytes: &FontData,
  font_refs: &FontRefs,
) -> Result<FontMap<Font>, PdfGenError> {
  let font_configs = &config.font_configs;
  let fonts = FontType::ALL
    .iter()
    .map(|font_type| {
      let font_config = font_configs.get(*font_type);
      let font_data = font_bytes.get(*font_type);
      let font_ref = font_refs.get(*font_type);

      let font = match font_ref.fvar() {
        Ok(_) => {
          let Some(axes_config) = font_config.variation_axes.as_ref() else {
            return Err(PdfGenError::MissingVariationAxes {
              font_type: *font_type,
            });
          };
          let axes = axes_config
            .iter()
            .map(|cfg_axis| {
              let tag = Tag::new(&cfg_axis.name);
              let value = cfg_axis.value as f32;
              let axis = (tag, value);
              return axis;
            })
            .collect::<Vec<_>>();
          Font::new_variable(font_data.clone().into(), font_config.font_index, &axes).ok_or(
            PdfGenError::FontCreation {
              font_type: *font_type,
            },
          )?
        },
        Err(ReadError::TableIsMissing(_)) => {
          Font::new(font_data.clone().into(), font_config.font_index).ok_or(PdfGenError::FontCreation {
            font_type: *font_type,
          })?
        },
        Err(source) => {
          return Err(PdfGenError::VariationTableRead {
            font_type: *font_type,
            source,
          });
        },
      };
      return Ok(font);
    })
    .collect::<Result<Vec<_>, PdfGenError>>()?;
  return Ok(FontMap::from_all(fonts));
}

/// レイアウト済みグリフ列を Krilla のグリフ列へ変換します。
///
/// Krilla の `KrillaGlyph` はメトリクス値を UPEM で正規化した値で受け取るため、
/// `layout::Glyph` の整数値を `upem` で除算して変換します。
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
