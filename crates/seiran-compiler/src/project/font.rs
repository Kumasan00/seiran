//! config.toml が宣言するフォント資源 — 19 種別の分類・実体（種別 × 候補）の鍵と表・検証済み設定・読込済みバイト列。
//!
//! フォントの解析・検証・シェーピングという**処理**は `crate::typeset::font` の側にあり、
//! この module はその入力契約（[`FontConfigs`]）と素材（[`FontData`]）までを持つ。
//!
//! TOML に対応する未検証型（`RawFontConfig` 等）とそこから検証済み値を構築する処理は
//! 兄弟 module `project::config` が持つ。

mod face_map;
mod font_type;
mod map;
mod settings;

use std::{collections::HashMap, fmt, sync::Arc};

pub(crate) use face_map::FaceMap;
pub use font_type::{FontFace, FontType};
pub(crate) use map::FontMap;
use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
pub(super) use settings::FontFeature;
pub use settings::VariationAxis;
pub(crate) use settings::{FontConfig, FontConfigs, TextDirection};
use thiserror::Error;

use crate::{
  failures::{self, Failures},
  project::{ProjectPath, ProjectSource, ProjectSourceError},
};

/// フォントファイルを読み込めないときのエラー。
#[derive(Debug, Error, Diagnostic)]
#[error("{face} のフォントファイルの読み込みに失敗しました: {path}")]
#[diagnostic(code(project::font::read), help("フォントファイルのパスと読み取り権限を確認してください。"))]
pub(crate) struct ReadFontError {
  /// このパスを指す最初の実体（[`FaceMap::faces`] の順）
  face: FontFace,
  /// ファイルパス
  path: String,
  /// 元の読み込みエラー（低水準 cause）
  #[source]
  source: ProjectSourceError,
}

/// 全実体（フォント種別 × 候補）のバイナリデータ。
///
/// 構築経路は [`FontData::load`] だけで、`ProjectSource` seam を必ず経由する。
///
/// バイト列は `Arc` で共有する — 同じフォントファイルを指す実体は同一の `Arc` を持ち、
/// 描画資源（`crate::publication`）へ渡すときもバイト列を複製しない。
#[derive(Clone, PartialEq)]
pub(crate) struct FontData(FaceMap<Arc<[u8]>>);

/// 実体ごとのバイト列の長さだけを出し、中身は出さない。
impl fmt::Debug for FontData {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    let bytes_len = self.0.map(|_, bytes| return bytes.len());
    return formatter.debug_struct("FontData").field("bytes_len", &bytes_len).finish();
  }
}

impl FontData {
  /// 設定された全フォントファイルを読み込む。同じパスを指す実体は 1 回だけ読む。
  ///
  /// パスは互いに独立に読めるので、1 件目で打ち切らず全パスを読んで失敗を全件返す。報告順は
  /// パスの昇順（`unique_paths` は `sort` 済み）で、`par_iter` は `IndexedParallelIterator` なので
  /// `collect::<Vec<_>>()` が入力順を保証する — どのファイルの読込が先に完了したかは報告順に漏れない。
  /// パスは `PathResolver` で正規化済みなので、表記違いも 1 件に畳まれる。
  ///
  /// # Errors
  ///
  /// いずれかのファイルを読み込めない場合に [`ReadFontError`] をパス昇順で返す。
  pub(crate) fn load(source: &dyn ProjectSource, font_configs: &FontConfigs) -> Result<Self, Failures<ReadFontError>> {
    let mut unique_paths: Vec<ProjectPath> =
      font_configs.faces().map(|face| return font_configs[face].font_path.clone()).collect();
    unique_paths.sort();
    unique_paths.dedup();

    let results = unique_paths
      .par_iter()
      .map(|path| {
        let bytes = source.read_bytes(path).map_err(|source| {
          let face = font_configs
            .faces()
            .find(|&face| return &font_configs[face].font_path == path)
            .expect("unique_paths は font_configs から集めた値のはず");
          return ReadFontError {
            face,
            path: path.to_string(),
            source,
          };
        })?;
        return Ok((path.clone(), bytes));
      })
      .collect::<Vec<Result<(ProjectPath, Arc<[u8]>), ReadFontError>>>();
    let loaded: HashMap<ProjectPath, Arc<[u8]>> = failures::collect_in_input_order(results)?.into_iter().collect();

    return Ok(FontData(font_configs.map(|_, font_config| {
      return Arc::clone(loaded.get(&font_config.font_path).expect("上のループで全パスを読み込み済みのはず"));
    })));
  }

  /// 指定された実体のバイト列を返す。
  #[must_use]
  pub(crate) fn bytes(&self, face: FontFace) -> &[u8] { return &self.0[face]; }

  /// 指定された実体のバイト列を共有ハンドルとして返す。
  #[must_use]
  pub(crate) fn shared_bytes(&self, face: FontFace) -> Arc<[u8]> { return Arc::clone(&self.0[face]); }
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;

  use super::{FaceMap, FontConfig, FontConfigs, FontData, FontFace, FontMap, FontType};
  use crate::project::{MemoryProjectSource, ProjectPath};

  /// `path` を指す、軸・タグ等を持たない設定。
  fn font_config(path: &str) -> FontConfig {
    return FontConfig {
      font_path: ProjectPath::new(path),
      font_index: 0,
      variation_axes: None,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  /// 全 19 種別の先頭候補が `shared_path` を指し、`serif` だけ `serif_extra` の候補を後ろに足した `FontConfigs` fixture を作る。
  fn make_font_configs_with_serif_extra(shared_path: &str, serif_extra: &[&str]) -> FontConfigs {
    let slots = FontMap::from_fn(|font_type| {
      let mut candidates = vec![font_config(shared_path)];
      if font_type == FontType::Serif {
        candidates.extend(serif_extra.iter().map(|path| return font_config(path)));
      }
      return candidates;
    });
    return FaceMap::from_slots(slots).expect("本文は 1 候補以上・math は 1 候補なので形は正しいはず");
  }

  /// 全 19 種別が同じ `shared_path` を指す `FontConfigs` fixture を作る。
  fn make_font_configs(shared_path: &str) -> FontConfigs {
    return make_font_configs_with_serif_extra(shared_path, &[]);
  }

  #[test]
  fn load_reads_shared_font_path_only_once() {
    let source = MemoryProjectSource::new().with_bytes("/fonts/shared.ttf", b"FAKE".to_vec());
    let font_configs = make_font_configs("/fonts/shared.ttf");

    let font_data = FontData::load(&source, &font_configs).expect("読み込めるはず");

    assert_eq!(source.read_count("/fonts/shared.ttf"), 1, "共有パスは 1 回しか読まれないはず");
    for face in font_configs.faces() {
      assert_eq!(font_data.bytes(face), b"FAKE");
    }
  }

  #[test]
  fn load_reads_a_path_shared_by_several_candidates_once() {
    let source = MemoryProjectSource::new().with_bytes("/fonts/shared.ttf", b"FAKE".to_vec());
    let font_configs = make_font_configs_with_serif_extra("/fonts/shared.ttf", &["/fonts/shared.ttf"]);

    let font_data = FontData::load(&source, &font_configs).expect("読み込めるはず");

    assert_eq!(source.read_count("/fonts/shared.ttf"), 1, "全候補が同じパスなら 1 回しか読まれないはず");
    assert_eq!(font_data.bytes(FontFace::new(FontType::Serif, 1)), b"FAKE");
  }

  #[test]
  fn debug_shows_byte_lengths_by_face_instead_of_contents() {
    // 中身が `70, 65, 75, 69` と整形されうる 4 バイトを全実体に入れる
    let source = MemoryProjectSource::new().with_bytes("/fonts/shared.ttf", b"FAKE".to_vec());
    let font_data = FontData::load(&source, &make_font_configs("/fonts/shared.ttf")).expect("読み込めるはず");

    let text = format!("{font_data:?}");

    assert!(
      text.starts_with("FontData { bytes_len: {serif[0]: 4, serif_bold[0]: 4, "),
      "実体をキーに長さだけが宣言順で出るはず: {text}"
    );
    assert!(!text.contains("70, 65"), "バイト列の中身は出ないはず: {text}");
  }

  #[test]
  fn load_reports_missing_font_with_its_face() {
    let source = MemoryProjectSource::new();
    let font_configs = make_font_configs("/fonts/missing.ttf");

    let result = FontData::load(&source, &font_configs);

    let Err(failures) = result else {
      panic!("フォントの読込エラーを期待");
    };
    assert_eq!(failures.first().face, FontFace::primary(FontType::Serif), "パスを指す最初の実体が報告されるはず");
    assert!(
      failures.first().to_string().starts_with("serif[0] のフォントファイル"),
      "実体は config.toml の位置で出るはず: {}",
      failures.first()
    );
    assert_eq!(
      failures.first().help().map(|help| return help.to_string()),
      Some("フォントファイルのパスと読み取り権限を確認してください。".to_string())
    );
  }

  #[test]
  fn load_reports_a_missing_second_candidate_with_its_index() {
    let source = MemoryProjectSource::new().with_bytes("/fonts/shared.ttf", b"FAKE".to_vec());
    let font_configs = make_font_configs_with_serif_extra("/fonts/shared.ttf", &["/fonts/missing.ttf"]);

    let failures = FontData::load(&source, &font_configs).expect_err("2 番目の候補が読めないはず");

    assert_eq!(failures.first().face, FontFace::new(FontType::Serif, 1));
  }
}
