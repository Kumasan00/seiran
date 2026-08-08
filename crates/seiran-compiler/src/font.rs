//! フォント処理エンジン
//!
//! 全フォント種別の読み込み・OpenType 解析・メトリクス取得を担い、シェイピングと
//! 設定検証を各サブモジュールで提供する。フォントのサブセット化は `krilla` に委ねる。
//!
//! フォント分類の型（`FontKind` / `FontType` と、全 19 種別が揃っていることを保証する `FontMap`）と、
//! フォント処理の入力契約となる処理済みフォント設定（`FontConfig` / `FontConfigs` / `VariationAxis` /
//! `Feature` / `TextDirection`）もこの module が所有する（#336）。前者は分類とその全域性の不変条件、
//! 後者は「後段が要求する入力契約は後段が所有する」という配置規則による。TOML に対応する未検証型と
//! そこから検証済み値を構築する処理は `crate::config` に残り、`font` は設定ファイルの形を知らない。

use std::{collections::HashMap, path::PathBuf};

use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;

use crate::font::map::FontMap;

mod face_config;
mod glyph_run;
mod kind;
mod map;
mod settings;
// `crate::typeset::block` 等が `shaper::UnicodeBuffer` を直接参照するため、`font` module 内に
// 閉じない可視性が必要。`seiran` の公開 API（lib.rs の `pub use`）には出さないため `pub` ではなく
// `pub(crate)` に留める（CLAUDE.md の「モジュール名が名前空間として意味を持つ場合のみ pub mod」の
// 例外に該当するが、吸収後は crate 外への公開経路を持たせない、#307 Task 7）。
pub(crate) mod shaper;
mod system;
mod validate_font;

// `face_config` / `map` / `validate_font` の型は `FontResources` のフィールド型・`FontSystemError` の
// 内部型・`FontData` 等の型エイリアスの実体として生きているが、`font` の外から名指しする消費者は
// いないので再エクスポートしない（`font` 内からは `map::FontMap` のように子 module のパスで参照する。
// 必要になったらここに足す）。
pub use glyph_run::{Glyph, GlyphRun};
// フォント分類（`FontKind` / `FontType`）は `font` が所有する（#336、旧 `model`）。
pub use kind::{FontKind, FontType};
// 処理済みフォント設定は font の入力契約なので font が所有する（#336、旧 `config`）。
// `config` が TOML の未検証型からこれらを構築するため、構築に名指しされる型はすべて facade に出す。
// `TextDirectionParseError` は `TextDirection::from_str` の `Err` 型としてしか現れず、名指しする
// 消費者がいないので facade へは出さない（#326）。
pub use settings::{Feature, FontConfig, FontConfigs, TextDirection, VariationAxis};
pub use system::{FontResources, FontSystem};

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
  fn new(source: &dyn crate::project::ProjectSource, font_configs: &FontConfigs) -> Result<Self, FontLoadError>;
}

impl FontDataExt for FontData {
  fn new(source: &dyn crate::project::ProjectSource, font_configs: &FontConfigs) -> Result<Self, FontLoadError> {
    let mut unique_paths: Vec<PathBuf> =
      FontType::ALL.iter().map(|&ft| return font_configs.get(ft).font_path.clone()).collect();
    unique_paths.sort();
    unique_paths.dedup();

    let loaded: HashMap<PathBuf, Vec<u8>> = unique_paths
      .par_iter()
      .map(|path| {
        let bytes = source.read_bytes(&crate::project::ProjectPath::new(path)).map_err(|source| {
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
  use super::*;
  use crate::project::MemoryProjectSource;

  /// 全 19 種別が同じ `shared_path` を指す `FontConfigs` fixture を作る。
  fn make_font_configs(shared_path: &str) -> FontConfigs {
    return FontConfigs::from_all(FontType::ALL.iter().map(|_| {
      return FontConfig {
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
