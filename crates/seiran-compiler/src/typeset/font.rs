//! フォント処理 — OpenType 解析・検証・メトリクス取得・シェイピング。
//!
//! 構築順序（解析 → メトリクス → 検証 → シェーパー）は子 module `system` に閉じる。

mod face_config;
mod math_script;
mod shaper;
mod stretch;
mod system;
mod validation;

use std::sync::Arc;

use harfrust::Font;
pub(super) use math_script::{ScriptLevel, ScriptScale};
use read_fonts::{FontRef, TableProvider};
pub(super) use shaper::Buffer;
use shaper::ShapingFonts;
pub(super) use stretch::VerticalStretch;
pub(super) use system::{FontSystem, FontSystemError};
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

/// バイナリデータから、設定されたフェースのフォント参照と、同じフェースのシェイピング用フォントを生成する。
///
/// フォント参照は解析の診断・メトリクス・検証に使い、シェイピング用フォントはバイト列を `FontData` と共有して持ち続ける。
///
/// # Errors
///
/// フォントを解析できない場合、または TTC のインデックスが範囲外の場合に
/// [`FontLoadError::ParseFont`] を `FontType` の宣言順で返す。
fn parse_fonts<'a>(
  config: &FontConfigs,
  font_data: &'a FontData,
) -> Result<(FontRefs<'a>, ShapingFonts), Failures<FontLoadError>> {
  let parsed = FontMap::par_try_from_fn(|font_type| {
    let font_config = &config[font_type];
    let index = font_config.font_index;
    let font_ref = FontRef::from_index(font_data.bytes(font_type), index).map_err(|source| {
      return FontLoadError::ParseFont {
        font_type,
        index,
        source,
      };
    })?;
    // `Arc<[u8]>` は既に unsized なので `dyn AsRef<[u8]>` へ直接は unsize できず、もう 1 段 `Arc` で包む
    let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(font_data.shared_bytes(font_type));
    let font = Font::new(blob, index).expect(
      "直前の FontRef::from_index が同じバイト列・同じ index で成功しており、Font::new も同じ解析で sfnt と認める",
    );
    return Ok((font_ref, shaper::at_configured_location(font, font_config)));
  })?;
  return Ok(parsed.unzip());
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
