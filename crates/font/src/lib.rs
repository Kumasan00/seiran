//! フォント処理エンジン
//!
//! 全フォント種別の読み込み・OpenType 解析・メトリクス取得を担い、シェイピングと
//! 設定検証を各サブモジュールで提供する。フォントのサブセット化は `krilla` に委ねる。

use std::fs;

use config::FontConfigs;
use miette::Diagnostic;
use model::{FontMap, FontType};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;

mod glyph_run;
pub mod shaper;
mod system;
mod validate_font;

pub use glyph_run::{Glyph, GlyphRun};
pub use system::{FontResources, FontSystem, FontSystemError};
pub use validate_font::{FontValidationError, FontValidationErrors, MultipleFontValidationErrors};

/// フォントの読み込み・解析エラー。
#[derive(Debug, Error, Diagnostic)]
pub enum FontLoadError {
  /// フォントファイルを読み込めない。
  #[error("{font_type:?} のフォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(font::load::read), help("フォントファイルのパスと読み取り権限を確認してください。"))]
  ReadFont {
    /// フォント種別
    font_type: FontType,
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// フォントを解析できない。
  #[error("{font_type:?} のフォント解析に失敗しました (index: {index})")]
  #[diagnostic(
    code(font::load::parse),
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
  #[error("{font_type:?} の {table} テーブルの読み込みに失敗しました")]
  #[diagnostic(
    code(font::load::metrics_table),
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

/// 全フォント種別のバイナリデータ。
pub type FontData = FontMap<Vec<u8>>;

/// [`FontData`] の構築機能。
pub trait FontDataExt: Sized {
  /// 設定された全フォントファイルを並列に読み込む。
  ///
  /// # Errors
  ///
  /// いずれかのファイルを読み込めない場合に [`FontLoadError::ReadFont`] を返す。
  fn new(font_configs: &FontConfigs) -> Result<Self, FontLoadError>;
}

impl FontDataExt for FontData {
  fn new(font_configs: &FontConfigs) -> Result<Self, FontLoadError> {
    let font_datas = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let font_config = font_configs.get(font_type);
        let font_path = &font_config.font_path;
        return fs::read(font_path).map_err(|source| {
          return FontLoadError::ReadFont {
            font_type,
            path: font_path.display().to_string(),
            source,
          };
        });
      })
      .collect::<Result<Vec<Vec<u8>>, FontLoadError>>()?;
    return Ok(FontMap::from_all(font_datas));
  }
}

/// 全フォント種別の解析済み OpenType フォント参照。
pub type FontRefs<'a> = FontMap<FontRef<'a>>;

/// [`FontRefs`] の構築機能。
pub trait FontRefsExt<'a>: Sized {
  /// バイナリデータから設定されたフェースのフォント参照を生成する。
  ///
  /// # Errors
  ///
  /// フォントを解析できない場合、または TTC のインデックスが範囲外の場合に
  /// [`FontLoadError::ParseFont`] を返す。
  fn new(config: &'a FontConfigs, font_data: &'a FontData) -> Result<Self, FontLoadError>;
}

impl<'a> FontRefsExt<'a> for FontRefs<'a> {
  fn new(config: &'a FontConfigs, font_data: &'a FontData) -> Result<Self, FontLoadError> {
    let font_refs = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let font_data = font_data.get(font_type);
        let font_config = config.get(font_type);
        let index = font_config.font_index;
        return FontRef::from_index(font_data, index).map_err(|source| {
          return FontLoadError::ParseFont {
            font_type,
            index,
            source,
          };
        });
      })
      .collect::<Result<Vec<FontRef<'a>>, FontLoadError>>()?;
    return Ok(FontMap::from_all(font_refs));
  }
}

/// 1 フォントの基本メトリクス。
///
/// 値はフォントユニット系で、`descender` は OpenType の慣例どおり通常は負値。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetric {
  /// units-per-em（`head` テーブル由来）
  pub upem: f32,
  /// アセンダ（`hhea` テーブル由来、フォントユニット）
  pub ascender: f32,
  /// ディセンダ（`hhea` テーブル由来、フォントユニット、通常は負値）
  pub descender: f32,
}

/// 全フォント種別の基本メトリクス。
pub type FontMetrics = FontMap<FontMetric>;

/// [`FontMetrics`] の構築機能。
pub trait FontMetricsExt: Sized {
  /// 全フォントの `head` / `hhea` テーブルからメトリクスを取得する。
  ///
  /// # Errors
  ///
  /// いずれかのテーブルを読めない場合に [`FontLoadError::ReadMetricsTable`] を返す。
  fn new(font_refs: &FontRefs) -> Result<Self, FontLoadError>;
}

impl FontMetricsExt for FontMetrics {
  fn new(font_refs: &FontRefs) -> Result<Self, FontLoadError> {
    let metrics = FontType::ALL
      .iter()
      .map(|&font_type| {
        let font_ref = font_refs.get(font_type);
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
        return Ok(FontMetric {
          upem: f32::from(head.units_per_em()),
          ascender: f32::from(hhea.ascender().to_i16()),
          descender: f32::from(hhea.descender().to_i16()),
        });
      })
      .collect::<Result<Vec<FontMetric>, FontLoadError>>()?;
    return Ok(FontMap::from_all(metrics));
  }
}
