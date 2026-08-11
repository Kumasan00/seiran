//! フォント処理 — OpenType 解析・検証・メトリクス取得・シェイピング。
//!
//! 入力は `project::font` が持つフォント資源（[`crate::project::FontConfigs`] と
//! [`crate::project::FontData`]）で、そこから解析済みフォント参照・メトリクス・シェーパーを
//! 組み立てる。フォントのサブセット化は `krilla` に委ねる。
//!
//! 外向きの interface は [`FontResources`] 1 型だけで、`FontRefs` / `FontMetrics` /
//! `FontSystem` / シェーパー / 検証は `typeset` の外から見えない。構築順序（解析 → メトリクス →
//! 検証 → シェーパー）は子 module `system` に閉じる（#352）。

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;

mod face_config;
mod glyph_run;
mod shaper;
mod system;
mod validate_font;

pub use face_config::{FontFaceConfig, VariationAxisConfig};
pub use glyph_run::{Glyph, GlyphRun};
// `shaper` module のパス自体は `typeset::font` に閉じ、`typeset::block` が shape 呼び出しに要る
// `UnicodeBuffer` だけを `typeset` 内へ出す。
pub(super) use shaper::UnicodeBuffer;
pub(super) use system::FontSystemError;
pub(crate) use system::{FontResources, FontSystem};
// フォント検証が集める warning。`compile` が `Warnings` へ載せるので `typeset` の外まで出す。
pub(crate) use validate_font::FontWarning;

use crate::{
  failures::{self, Failures},
  project::{FontConfigs, FontData, FontMap, FontType},
};

/// フォントの解析エラー。
///
/// 名指しするのは `system::FontSystemError::Load` だけで、`typeset::font` の facade には載せない
/// （`TypesetError` 越しに `pub(crate)` へ到達しうるので型自体の可視性はそこに合わせる）。
#[derive(Debug, Error, miette::Diagnostic)]
pub(crate) enum FontLoadError {
  /// フォントを解析できない。
  #[error("{font_type:?} のフォント解析に失敗しました (index: {index})")]
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
  #[error("{font_type:?} の {table} テーブルの読み込みに失敗しました")]
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
/// フォントは互いに独立に解析できるので、1 件目で打ち切らず全種別を解析して違反を全件返す。
/// `FontType::ALL.par_iter()` は `IndexedParallelIterator` なので `collect::<Vec<_>>()` が入力順を
/// 保証し、どのフォントの解析が先に完了したかは報告順に漏れない（`collect::<Result<Vec<_>, E>>()`
/// は複数エラーのうちどれを返すかが非決定的なので使わない）。
///
/// # Errors
///
/// フォントを解析できない場合、または TTC のインデックスが範囲外の場合に
/// [`FontLoadError::ParseFont`] を `FontType::ALL` 順で返す。
fn build_font_refs<'a>(
  config: &'a FontConfigs,
  font_data: &'a FontData,
) -> Result<FontRefs<'a>, Failures<FontLoadError>> {
  let results = FontType::ALL
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
    .collect::<Vec<Result<FontRef<'a>, FontLoadError>>>();
  return Ok(FontMap::from_all(failures::collect_in_input_order(results)?));
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
pub(crate) type FontMetrics = FontMap<FontMetric>;

/// 全フォントの `head` / `hhea` テーブルからメトリクスを取得する。
///
/// フォントは互いに独立にメトリクスを読めるので、1 件目で打ち切らず全種別を読んで違反を全件返す。
///
/// # Errors
///
/// いずれかのテーブルを読めない場合に [`FontLoadError::ReadMetricsTable`] を `FontType::ALL` 順で返す。
fn build_font_metrics(font_refs: &FontRefs) -> Result<FontMetrics, Failures<FontLoadError>> {
  let results = FontType::ALL
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
    .collect::<Vec<Result<FontMetric, FontLoadError>>>();
  return Ok(FontMap::from_all(failures::collect_in_input_order(results)?));
}
