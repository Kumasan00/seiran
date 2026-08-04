//! フォント処理エンジン
//!
//! 全フォント種別の読み込み・OpenType 解析・メトリクス取得を担い、シェイピングと
//! 設定検証を各サブモジュールで提供する。フォントのサブセット化は `krilla` に委ねる。

use std::{collections::HashMap, path::PathBuf};

use config::FontConfigs;
use miette::Diagnostic;
use model::{FontMap, FontType};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;

mod face_config;
mod glyph_run;
pub mod shaper;
mod system;
mod validate_font;

pub use face_config::{FontFaceConfig, FontFaceConfigs, VariationAxisConfig, build_face_configs};
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
    /// 元の読み込みエラー
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
  /// 設定された全フォントファイルを読み込む。同じパスを指す種別は 1 回だけ読む。
  ///
  /// # Errors
  ///
  /// いずれかのファイルを読み込めない場合に [`FontLoadError::ReadFont`] を返す。
  fn new(source: &dyn config::ProjectSource, font_configs: &FontConfigs) -> Result<Self, FontLoadError>;
}

impl FontDataExt for FontData {
  fn new(source: &dyn config::ProjectSource, font_configs: &FontConfigs) -> Result<Self, FontLoadError> {
    let mut unique_paths: Vec<PathBuf> =
      FontType::ALL.iter().map(|&ft| return font_configs.get(ft).font_path.clone()).collect();
    unique_paths.sort();
    unique_paths.dedup();

    let loaded: HashMap<PathBuf, Vec<u8>> = unique_paths
      .par_iter()
      .map(|path| {
        let bytes = source.read_bytes(&config::ProjectPath::new(path)).map_err(|source| {
          let font_type = FontType::ALL
            .iter()
            .find(|&&ft| return &font_configs.get(ft).font_path == path)
            .copied()
            .expect("unique_paths は font_configs から集めた値のはず");
          return FontLoadError::ReadFont {
            font_type,
            path: path.display().to_string(),
            source: source.into_io(),
          };
        })?;
        return Ok((path.clone(), bytes.to_vec()));
      })
      .collect::<Result<HashMap<_, _>, FontLoadError>>()?;

    let font_datas = FontType::ALL
      .iter()
      .map(|&font_type| {
        let path = &font_configs.get(font_type).font_path;
        return loaded.get(path).expect("上のループで全パスを読み込み済みのはず").clone();
      })
      .collect::<Vec<Vec<u8>>>();
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

#[cfg(test)]
mod tests {
  use config::{FontConfig, FontConfigs, MemoryProjectSource};
  use model::FontType;

  use super::*;

  /// 全 19 種別が同じ `shared_path` を指す `FontConfigs` fixture を作る。
  fn make_font_configs(shared_path: &str) -> FontConfigs {
    return FontConfigs::from_all(FontType::ALL.iter().map(|_| {
      return FontConfig {
        font_name: "test".to_string(),
        font_path: shared_path.into(),
        font_index: 0,
        variation_axes: None,
        script: None,
        language: None,
        ot_language_tag: None,
        direction: None,
        features: None,
      };
    }));
  }

  #[test]
  fn new_reads_shared_font_path_only_once() {
    // Arrange — 全 19 種別が同じフォントファイルを指す fixture
    let source = MemoryProjectSource::new().with_bytes("/fonts/shared.ttf", b"FAKE".to_vec());
    let font_configs = make_font_configs("/fonts/shared.ttf");

    // Act
    let font_data = FontData::new(&source, &font_configs).expect("読み込めるはず");

    // Assert — read_bytes は 1 回だけ呼ばれ、全 19 種別に同じ内容が入る
    assert_eq!(source.read_count("/fonts/shared.ttf"), 1, "共有パスは 1 回しか読まれないはず");
    for font_type in FontType::ALL {
      assert_eq!(font_data.get(font_type), b"FAKE");
    }
  }

  #[test]
  fn new_reports_missing_font_with_its_font_type() {
    // Arrange
    let source = MemoryProjectSource::new();
    let font_configs = make_font_configs("/fonts/missing.ttf");

    // Act
    let result = FontData::new(&source, &font_configs);

    // Assert
    assert!(matches!(result, Err(FontLoadError::ReadFont { .. })));
    let Err(FontLoadError::ReadFont { font_type, .. }) = result else {
      panic!("ReadFont を期待, got {result:?}");
    };
    assert_eq!(font_type, FontType::ALL[0], "唯一のフォント種別が報告されるはず");
  }
}
