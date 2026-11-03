//! フォント処理 — OpenType 解析・検証・メトリクス取得・シェイピング。
//!
//! 構築順序（解析 → メトリクス → 検証 → シェーパー）は子 module `system` に閉じる。

mod fallback;
mod math_script;
mod shaper;
mod stretch;
mod system;
mod validation;

use std::sync::Arc;

pub(super) use fallback::{Finding, is_variation_selector, shaped_glyphs};
use harfrust::Font;
pub(super) use math_script::{ScriptLevel, ScriptScale};
use read_fonts::{FontRef, TableProvider};
pub(super) use shaper::Buffer;
use shaper::ShapingFonts;
pub(super) use stretch::Stretch;
pub(super) use system::{FontSystem, FontSystemError};
use thiserror::Error;
pub(super) use validation::FontWarning;

use crate::{
  failures::Failures,
  project::{FaceMap, FontConfigs, FontData, FontFace},
  publication::FontMetrics,
};

/// フォントの解析エラー。
#[derive(Debug, Error, miette::Diagnostic)]
pub(crate) enum FontLoadError {
  /// フォントを解析できない。
  #[error("{face} のフォント解析に失敗しました (index: {index})")]
  #[diagnostic(
    code(typeset::font::parse),
    help(
      "フォントファイルが有効な OpenType フォントであることを確認してください。TTC の場合、font_index が正しいか確認してください。"
    )
  )]
  ParseFont {
    /// 実体（フォント種別 × 候補）
    face: FontFace,
    /// TTC 内のフォントインデックス
    index: u32,
    /// 元の解析エラー
    #[source]
    source: read_fonts::ReadError,
  },
  /// メトリクス取得に必要な OpenType テーブルを読めない。
  #[error("{face} の {table} テーブルの読み込みに失敗しました")]
  #[diagnostic(
    code(typeset::font::metrics_table),
    help("入力フォントが壊れていないか、font_index が正しいかを確認してください。")
  )]
  ReadMetricsTable {
    /// 実体（フォント種別 × 候補）
    face: FontFace,
    /// 読み込みに失敗したテーブル名（`head` / `hhea`）
    table: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: read_fonts::ReadError,
  },
}

/// 全実体の解析済み OpenType フォント参照。
type FontRefs<'a> = FaceMap<FontRef<'a>>;

/// バイナリデータから、設定されたフェースのフォント参照と、同じフェースのシェイピング用フォントを生成する。
///
/// フォント参照は解析の診断・メトリクス・検証に使い、シェイピング用フォントはバイト列を `FontData` と共有して持ち続ける。
///
/// # Errors
///
/// フォントを解析できない場合、または TTC のインデックスが範囲外の場合に
/// [`FontLoadError::ParseFont`] を実体の順（[`FaceMap::faces`]）で返す。
fn parse_fonts<'a>(
  config: &FontConfigs,
  font_data: &'a FontData,
) -> Result<(FontRefs<'a>, ShapingFonts), Failures<FontLoadError>> {
  let parsed = config.par_try_map(|face, font_config| {
    let index = font_config.font_index;
    let font_ref = FontRef::from_index(font_data.bytes(face), index).map_err(|source| {
      return FontLoadError::ParseFont {
        face,
        index,
        source,
      };
    })?;
    // `Arc<[u8]>` は既に unsized なので `dyn AsRef<[u8]>` へ直接は unsize できず、もう 1 段 `Arc` で包む
    let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(font_data.shared_bytes(face));
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
/// いずれかのテーブルを読めない場合に [`FontLoadError::ReadMetricsTable`] を実体の順（[`FaceMap::faces`]）で返す。
fn build_font_metrics(font_refs: &FontRefs<'_>) -> Result<FaceMap<FontMetrics>, Failures<FontLoadError>> {
  return font_refs.try_map(|face, font_ref| {
    let head = font_ref.head().map_err(|source| {
      return FontLoadError::ReadMetricsTable {
        face,
        table: "head",
        source,
      };
    })?;
    let hhea = font_ref.hhea().map_err(|source| {
      return FontLoadError::ReadMetricsTable {
        face,
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

/// 本文用フォントの上付きの指標（OS/2 の `ySuperscriptYSize` / `ySuperscriptYOffset` を em に対する比にしたもの）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::typeset) struct SuperscriptMetrics {
  /// 上付きの字の大きさ（em に対する比。正）
  pub size: f64,
  /// 上付きのベースラインからの持ち上げ（em に対する比。正で上）
  pub offset: f64,
}

/// フォント `font_ref`（units-per-em は `upem`）の OS/2 から上付きの指標を読む
///
/// OS/2 テーブルを読めないか、`ySuperscriptYSize` が 0 以下なら `None`（既定の比率へ黙って落とさず、使う側が診断する）。
fn superscript_metrics(font_ref: &FontRef<'_>, upem: f32) -> Option<SuperscriptMetrics> {
  let os2 = font_ref.os2().ok()?;
  let size = os2.y_superscript_y_size();
  if size <= 0 {
    return None;
  }
  let upem = f64::from(upem);
  return Some(SuperscriptMetrics {
    size: f64::from(size) / upem,
    offset: f64::from(os2.y_superscript_y_offset()) / upem,
  });
}

#[cfg(test)]
mod tests {
  use std::{fs, path::Path};

  use read_fonts::FontRef;

  use super::{FontLoadError, superscript_metrics};
  use crate::project::{FontFace, FontType};

  /// `OS/2` テーブル（version 0 の 78 バイト）だけを持ち、`ySuperscriptYSize` / `ySuperscriptYOffset` が `size` / `offset`
  /// の sfnt バイト列を組む
  fn sfnt_with_os2(size: i16, offset: i16) -> Vec<u8> {
    let mut os2 = vec![0u8; 78];
    os2[20..22].copy_from_slice(&size.to_be_bytes());
    os2[24..26].copy_from_slice(&offset.to_be_bytes());
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfntVersion
    bytes.extend_from_slice(&1u16.to_be_bytes()); // numTables
    bytes.extend_from_slice(&[0; 6]); // searchRange / entrySelector / rangeShift
    bytes.extend_from_slice(b"OS/2");
    bytes.extend_from_slice(&0u32.to_be_bytes()); // checksum
    bytes.extend_from_slice(&28u32.to_be_bytes()); // offset（ヘッダ 12 バイト + レコード 16 バイト）
    bytes.extend_from_slice(&78u32.to_be_bytes()); // length
    bytes.extend_from_slice(&os2);
    return bytes;
  }

  #[test]
  fn superscript_metrics_are_read_as_em_ratios() {
    let bytes = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoText[wght].ttf"))
      .expect("vendor/fonts のフォントを読めるはず（未取得なら tools/fetch-test-assets.sh）");
    let font = FontRef::new(&bytes).unwrap();

    let metrics = superscript_metrics(&font, 1000.0).expect("STIX Two Text は OS/2 の上付き指標を持つ");

    assert!(metrics.size > 0.0 && metrics.size < 1.0, "{metrics:?}");
    assert!(metrics.offset > 0.0 && metrics.offset < 1.0, "{metrics:?}");
  }

  #[test]
  fn superscript_metrics_divide_by_upem() {
    let bytes = sfnt_with_os2(650, 400);

    let metrics = superscript_metrics(&FontRef::new(&bytes).unwrap(), 1000.0).unwrap();

    assert!((metrics.size - 0.65).abs() < 1e-9 && (metrics.offset - 0.4).abs() < 1e-9, "{metrics:?}");
  }

  #[test]
  fn missing_or_non_positive_superscript_size_is_none() {
    let no_tables = [0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    let zero = sfnt_with_os2(0, 400);
    let negative = sfnt_with_os2(-1, 400);

    assert_eq!(superscript_metrics(&FontRef::new(&no_tables).unwrap(), 1000.0), None, "OS/2 が無い");
    assert_eq!(superscript_metrics(&FontRef::new(&zero).unwrap(), 1000.0), None);
    assert_eq!(superscript_metrics(&FontRef::new(&negative).unwrap(), 1000.0), None);
  }

  #[test]
  fn messages_name_the_face_by_its_config_position() {
    let parse = FontLoadError::ParseFont {
      face: FontFace::new(FontType::SansSerifBold, 1),
      index: 0,
      source: read_fonts::ReadError::OutOfBounds,
    };
    let metrics = FontLoadError::ReadMetricsTable {
      face: FontFace::primary(FontType::SansSerifBold),
      table: "head",
      source: read_fonts::ReadError::OutOfBounds,
    };

    assert!(parse.to_string().starts_with("sans_serif_bold[1] "), "{parse}");
    assert!(metrics.to_string().starts_with("sans_serif_bold[0] "), "{metrics}");
  }
}
