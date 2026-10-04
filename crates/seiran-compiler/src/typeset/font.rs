//! フォント処理 — OpenType 解析・検証・メトリクス取得・シェイピング。
//!
//! 構築順序（解析 → メトリクス → 検証 → シェーパー）は子 module `system` に閉じる。

mod face_config;
mod shaper;
mod system;
mod validation;

use read_fonts::{FontRef, TableProvider};
pub(super) use shaper::UnicodeBuffer;
pub(super) use system::{FontResources, FontSystem, FontSystemError};
use thiserror::Error;
pub(super) use validation::FontWarning;

use crate::{
  failures::Failures,
  project::{FontConfigs, FontData, FontMap, FontType},
  publication::FontMetrics,
};

/// フォントの解析エラー。
#[derive(Debug, Error, miette::Diagnostic)]
pub(crate) enum FontLoadError {
  /// フォントを解析できない。
  #[error("{} のフォント解析に失敗しました (index: {index})", .font_type.as_toml_key())]
  #[diagnostic(
    code(typeset::font::parse),
    help(
      "フォントファイルが有効な OpenType フォントであることを確認してください。TTC の場合、font_index が正しいか確認してください。"
    )
  )]
  ParseFont {
    /// フォント種別
    font_type: FontType,
    /// TTC 内のフォントインデックス
    index: u32,
    /// 元の解析エラー
    #[source]
    source: read_fonts::ReadError,
  },
  /// メトリクス取得に必要な OpenType テーブルを読めない。
  #[error("{} の {table} テーブルの読み込みに失敗しました", .font_type.as_toml_key())]
  #[diagnostic(
    code(typeset::font::metrics_table),
    help("入力フォントが壊れていないか、font_index が正しいかを確認してください。")
  )]
  ReadMetricsTable {
    /// フォント種別
    font_type: FontType,
    /// 読み込みに失敗したテーブル名（`head` / `hhea`）
    table: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: read_fonts::ReadError,
  },
}

/// 全フォント種別の解析済み OpenType フォント参照。
type FontRefs<'a> = FontMap<FontRef<'a>>;

/// バイナリデータから設定されたフェースのフォント参照を生成する。
///
/// # Errors
///
/// フォントを解析できない場合、または TTC のインデックスが範囲外の場合に
/// [`FontLoadError::ParseFont`] を `FontType` の宣言順で返す。
fn build_font_refs<'a>(
  config: &'a FontConfigs,
  font_data: &'a FontData,
) -> Result<FontRefs<'a>, Failures<FontLoadError>> {
  return FontMap::par_try_from_fn(|font_type| {
    let font_data = font_data.bytes(font_type);
    let font_config = &config[font_type];
    let index = font_config.font_index;
    return FontRef::from_index(font_data, index).map_err(|source| {
      return FontLoadError::ParseFont {
        font_type,
        index,
        source,
      };
    });
  });
}

/// 全フォントの `head` / `hhea` テーブルからメトリクスを取得する。
///
/// # Errors
///
/// いずれかのテーブルを読めない場合に [`FontLoadError::ReadMetricsTable`] を `FontType` の宣言順で返す。
fn build_font_metrics(font_refs: &FontRefs<'_>) -> Result<FontMap<FontMetrics>, Failures<FontLoadError>> {
  return FontMap::try_from_fn(|font_type| {
    let font_ref = &font_refs[font_type];
    let head = font_ref.head().map_err(|source| {
      return FontLoadError::ReadMetricsTable {
        font_type,
        table: "head",
        source,
      };
    })?;
    let hhea = font_ref.hhea().map_err(|source| {
      return FontLoadError::ReadMetricsTable {
        font_type,
        table: "hhea",
        source,
      };
    })?;
    return Ok(FontMetrics {
      upem: f32::from(head.units_per_em()),
      ascender: f32::from(hhea.ascender().to_i16()),
      descender: f32::from(hhea.descender().to_i16()),
    });
  });
}

#[cfg(test)]
mod tests {
  use super::FontLoadError;
  use crate::project::FontType;

  #[test]
  fn messages_name_the_font_type_by_its_config_key() {
    let parse = FontLoadError::ParseFont {
      font_type: FontType::SansSerifBold,
      index: 0,
      source: read_fonts::ReadError::OutOfBounds,
    };
    let metrics = FontLoadError::ReadMetricsTable {
      font_type: FontType::SansSerifBold,
      table: "head",
      source: read_fonts::ReadError::OutOfBounds,
    };

    assert!(parse.to_string().starts_with("sans_serif_bold "), "{parse}");
    assert!(metrics.to_string().starts_with("sans_serif_bold "), "{metrics}");
  }
}
