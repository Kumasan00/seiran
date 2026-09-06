//! config.toml が宣言するフォント資源 — 19 種別の分類・検証済み設定・読込済みバイト列。
//!
//! 「どのフォントファイルを使うか」はプロジェクトの物理的な入力なので、この module が所有する。
//! フォントの解析・検証・シェーピングという**処理**は `crate::typeset::font` の側にあり、
//! この module はその入力契約（[`FontConfigs`]）と素材（[`FontData`]）までを持つ（#352）。
//!
//! TOML に対応する未検証型（`RawFontConfig` 等）とそこから検証済み値を構築する処理は
//! 兄弟 module `project::config` が持つ。

use std::{collections::HashMap, sync::Arc};

use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use thiserror::Error;

mod kind;
mod map;
mod settings;

pub use kind::FontType;
pub(crate) use map::FontMap;
pub(crate) use settings::{Feature, FontConfig, FontConfigs, TextDirection, VariationAxis};

use crate::{
  failures::{self, Failures},
  project::{ProjectPath, ProjectSource, SourceReadError},
};

/// フォントファイルを読み込めないときのエラー。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum FontReadError {
  /// フォントファイルを読み込めない。
  #[error("{font_type:?} のフォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(project::font::read), help("フォントファイルのパスと読み取り権限を確認してください。"))]
  ReadFont {
    /// フォント種別
    font_type: FontType,
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー（低水準 cause）
    #[source]
    source: SourceReadError,
  },
}

/// 全フォント種別のバイナリデータ。
///
/// 構築経路は [`FontData::load`] だけで、`ProjectSource` seam を必ず経由する。
///
/// バイト列は `Arc` で共有する — 同じフォントファイルを指す種別は同一の `Arc` を持ち、
/// 描画資源（`crate::publication`）へ渡すときもバイト列を複製しない。seam
/// （[`ProjectSource::read_bytes`]）が返す `Arc<[u8]>` をそのまま持つのはこのためで、
/// `Vec` へ移し替えると seam のキャッシュと二重に常駐する。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontData(FontMap<Arc<[u8]>>);

impl FontData {
  /// 設定された全フォントファイルを読み込む。同じパスを指す種別は 1 回だけ読む。
  ///
  /// パスは互いに独立に読めるので、1 件目で打ち切らず全パスを読んで失敗を全件返す。報告順は
  /// パスの昇順（`unique_paths` は `sort` 済み）で、`par_iter` は `IndexedParallelIterator` なので
  /// `collect::<Vec<_>>()` が入力順を保証する — どのファイルの読込が先に完了したかは報告順に漏れない。
  /// パスは `PathResolver` で正規化済みなので、表記違いは 1 件に畳まれ、同じファイルを 2 回読まない。
  ///
  /// # Errors
  ///
  /// いずれかのファイルを読み込めない場合に [`FontReadError::ReadFont`] をパス昇順で返す。
  pub(crate) fn load(source: &dyn ProjectSource, font_configs: &FontConfigs) -> Result<Self, Failures<FontReadError>> {
    let mut unique_paths: Vec<ProjectPath> =
      FontType::ALL.iter().map(|&ft| return font_configs[ft].font_path.clone()).collect();
    unique_paths.sort();
    unique_paths.dedup();

    let results = unique_paths
      .par_iter()
      .map(|path| {
        let bytes = source.read_bytes(path).map_err(|source| {
          let font_type = FontType::ALL
            .iter()
            .find(|&&ft| return &font_configs[ft].font_path == path)
            .copied()
            .expect("unique_paths は font_configs から集めた値のはず");
          return FontReadError::ReadFont {
            font_type,
            path: path.to_string(),
            source,
          };
        })?;
        return Ok((path.clone(), bytes));
      })
      .collect::<Vec<Result<(ProjectPath, Arc<[u8]>), FontReadError>>>();
    let loaded: HashMap<ProjectPath, Arc<[u8]>> = failures::collect_in_input_order(results)?.into_iter().collect();

    let font_datas = FontType::ALL
      .iter()
      .map(|&font_type| {
        let path = &font_configs[font_type].font_path;
        return Arc::clone(loaded.get(path).expect("上のループで全パスを読み込み済みのはず"));
      })
      .collect::<Vec<Arc<[u8]>>>();
    return Ok(FontData(FontMap::from_all(font_datas)));
  }

  /// 指定されたフォント種別のバイト列を返す。
  #[must_use]
  pub(crate) fn get(&self, font_type: FontType) -> &[u8] { return &self.0[font_type]; }

  /// 指定されたフォント種別のバイト列を共有ハンドルとして返す。
  ///
  /// `Publication` の描画資源へ渡すために使う（バイト列は複製されない）。
  #[must_use]
  pub(crate) fn shared_bytes(&self, font_type: FontType) -> Arc<[u8]> { return Arc::clone(&self.0[font_type]); }
}

#[cfg(test)]
mod tests {
  use super::{FontConfig, FontConfigs, FontData, FontReadError, FontType};
  use crate::project::{MemoryProjectSource, ProjectPath};

  /// 全 19 種別が同じ `shared_path` を指す `FontConfigs` fixture を作る。
  fn make_font_configs(shared_path: &str) -> FontConfigs {
    return FontConfigs::from_all(FontType::ALL.iter().map(|_| {
      return FontConfig {
        font_path: ProjectPath::new(shared_path),
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
    let Err(failures) = result else {
      panic!("ReadFont を期待");
    };
    let FontReadError::ReadFont { font_type, .. } = failures.first();
    assert_eq!(*font_type, FontType::ALL[0], "唯一のフォント種別が報告されるはず");
  }
}
