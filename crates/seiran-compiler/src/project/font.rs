//! config.toml が宣言するフォント資源 — 19 種別の分類・検証済み設定・読込済みバイト列。
//!
//! 「どのフォントファイルを使うか」はプロジェクトの物理的な入力なので、この module が所有する。
//! フォントの解析・検証・シェーピングという**処理**は `crate::typeset::font` の側にあり、
//! この module はその入力契約（[`FontConfigs`]）と素材（[`FontData`]）までを持つ（#352）。
//!
//! TOML に対応する未検証型（`PreFontConfig` 等）とそこから検証済み値を構築する処理は
//! 兄弟 module `project::config` が持つ。

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use thiserror::Error;

mod kind;
mod map;
mod settings;

pub use kind::FontType;
pub use map::FontMap;
pub use settings::{Feature, FontConfig, FontConfigs, TextDirection, VariationAxis};

use crate::project::{ProjectPath, ProjectSource};

/// フォントファイルを読み込めないときのエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum FontReadError {
  /// フォントファイルを読み込めない。
  #[error("{font_type:?} のフォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(project::font::read), help("フォントファイルのパスと読み取り権限を確認してください。"))]
  ReadFont {
    /// フォント種別
    font_type: FontType,
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー
    #[source]
    source: std::io::Error,
  },
}

/// 全フォント種別のバイナリデータ。
///
/// 構築経路は [`FontData::load`] だけで、`ProjectSource` seam を必ず経由する。
///
/// バイト列は `Arc` で共有する — 同じフォントファイルを指す種別は同一の `Arc` を持ち、
/// 描画資源（`crate::publication`）へ渡すときもバイト列を複製しない。
#[derive(Debug, Clone, PartialEq)]
pub struct FontData(FontMap<Arc<Vec<u8>>>);

impl FontData {
  /// 設定された全フォントファイルを読み込む。同じパスを指す種別は 1 回だけ読む。
  ///
  /// # Errors
  ///
  /// いずれかのファイルを読み込めない場合に [`FontReadError::ReadFont`] を返す。
  pub fn load(source: &dyn ProjectSource, font_configs: &FontConfigs) -> Result<Self, FontReadError> {
    let mut unique_paths: Vec<PathBuf> =
      FontType::ALL.iter().map(|&ft| return font_configs.get(ft).font_path.clone()).collect();
    unique_paths.sort();
    unique_paths.dedup();

    let loaded: HashMap<PathBuf, Arc<Vec<u8>>> = unique_paths
      .par_iter()
      .map(|path| {
        let bytes = source.read_bytes(&ProjectPath::new(path)).map_err(|source| {
          let font_type = FontType::ALL
            .iter()
            .find(|&&ft| return &font_configs.get(ft).font_path == path)
            .copied()
            .expect("unique_paths は font_configs から集めた値のはず");
          return FontReadError::ReadFont {
            font_type,
            path: path.display().to_string(),
            source: source.into_io(),
          };
        })?;
        return Ok((path.clone(), Arc::new(bytes.to_vec())));
      })
      .collect::<Result<HashMap<_, _>, FontReadError>>()?;

    let font_datas = FontType::ALL
      .iter()
      .map(|&font_type| {
        let path = &font_configs.get(font_type).font_path;
        return Arc::clone(loaded.get(path).expect("上のループで全パスを読み込み済みのはず"));
      })
      .collect::<Vec<Arc<Vec<u8>>>>();
    return Ok(FontData(FontMap::from_all(font_datas)));
  }

  /// 指定されたフォント種別のバイト列を返す。
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &[u8] { return self.0.get(font_type).as_slice(); }

  /// 指定されたフォント種別のバイト列を共有ハンドルとして返す。
  ///
  /// `Publication` の描画資源へ渡すために使う（バイト列は複製されない）。
  #[must_use]
  pub fn shared_bytes(&self, font_type: FontType) -> Arc<Vec<u8>> { return Arc::clone(self.0.get(font_type)); }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{FontConfig, FontConfigs, FontData, FontReadError, FontType};
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
  fn load_reads_shared_font_path_only_once() {
    // Arrange — 全 19 種別が同じフォントファイルを指す fixture
    let source = MemoryProjectSource::new().with_bytes("/fonts/shared.ttf", b"FAKE".to_vec());
    let font_configs = make_font_configs("/fonts/shared.ttf");

    // Act
    let font_data = FontData::load(&source, &font_configs).expect("読み込めるはず");

    // Assert — read_bytes は 1 回だけ呼ばれ、全 19 種別に同じ内容が入る
    assert_eq!(source.read_count("/fonts/shared.ttf"), 1, "共有パスは 1 回しか読まれないはず");
    for font_type in FontType::ALL {
      assert_eq!(font_data.get(font_type), b"FAKE");
    }
  }

  #[test]
  fn load_reports_missing_font_with_its_font_type() {
    // Arrange
    let source = MemoryProjectSource::new();
    let font_configs = make_font_configs("/fonts/missing.ttf");

    // Act
    let result = FontData::load(&source, &font_configs);

    // Assert
    assert!(matches!(result, Err(FontReadError::ReadFont { .. })));
    let Err(FontReadError::ReadFont { font_type, .. }) = result else {
      panic!("ReadFont を期待, got {result:?}");
    };
    assert_eq!(font_type, FontType::ALL[0], "唯一のフォント種別が報告されるはず");
  }
}
