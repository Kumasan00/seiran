//! render の入力となる、フォント・画像の確定済み資源をまとめる。

use std::collections::HashMap;

use font::{FontData, FontRefs};
use krilla::text::{Font, Tag};
use model::{AssetId, FontMap, FontType};
use read_fonts::{ReadError, TableProvider};

use crate::error::PdfGenError;

/// Krilla フォント構築に必要な設定（`config` クレートに依存しない最小表現）。
#[derive(Debug, Clone)]
pub struct FontResourceConfig {
  /// TTC（TrueType Collection）ファイル内のインデックス
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  pub variation_axes: Option<Vec<VariationAxisConfig>>,
}

/// バリアブルフォント軸の設定値（`config::VariationAxis` の複製）。
#[derive(Debug, Clone, Copy)]
pub struct VariationAxisConfig {
  /// 軸名（4 バイトの OpenType 軸タグ）
  pub name: [u8; 4],
  /// 目標値（実数）
  pub value: f64,
}

/// 19 フォント種別すべての [`FontResourceConfig`]。
pub type FontResourceConfigs = FontMap<FontResourceConfig>;

/// render に必要なフォント・画像資源一式。
///
/// `render` はこれ以外のフォント資源構築・ファイル読み込みを行わない。
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceBundle {
  /// フォント種別ごとの Krilla フォント（構築済み）
  pub(crate) fonts: FontMap<Font>,
  /// フォント種別ごとの計測値（構築済み）
  pub(crate) font_metrics: font::FontMetrics,
  /// 画像パスごとの生バイト列（未デコード）
  pub(crate) image_bytes: HashMap<AssetId, Vec<u8>>,
}

impl ResourceBundle {
  /// フォント・画像資源から [`ResourceBundle`] を構築する。
  ///
  /// # Errors
  ///
  /// バリアブルフォントに必要な軸設定が不足している、またはフォントの生成に失敗した場合に
  /// [`PdfGenError`] を返す。
  pub fn new(
    configs: &FontResourceConfigs,
    font_bytes: &FontData,
    font_refs: &FontRefs,
    font_metrics: font::FontMetrics,
    image_bytes: HashMap<AssetId, Vec<u8>>,
  ) -> Result<Self, PdfGenError> {
    let fonts = build_krilla_fonts(configs, font_bytes, font_refs)?;
    return Ok(ResourceBundle {
      fonts,
      font_metrics,
      image_bytes,
    });
  }
}

/// フォント設定に基づいて Krilla 用フォント集合を構築する。
fn build_krilla_fonts(
  configs: &FontResourceConfigs,
  font_bytes: &FontData,
  font_refs: &FontRefs,
) -> Result<FontMap<Font>, PdfGenError> {
  let fonts = FontType::ALL
    .iter()
    .map(|font_type| {
      let font_config = configs.get(*font_type);
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
              // krilla variable font 軸値は f32 のみ受け付ける（API 境界での精度低下は許容）
              #[allow(clippy::cast_possible_truncation)]
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
