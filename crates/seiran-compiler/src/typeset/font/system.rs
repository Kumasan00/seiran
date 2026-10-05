//! フォント資源の構築順序を隠蔽する窓口モジュール
//!
//! `FontData` → `FontRefs` / `ShapingFonts` → `FontMap<FontMetrics>` → 検証 → `HarfRustShapers`
//! という構築順序をここに閉じ込め、[`FontSystem::load`] でシェイプ可能な資源一式を返す。

use std::time::Instant;

use miette::Diagnostic;
use read_fonts::{TableProvider, tables::math::MathConstants};
use thiserror::Error;
use tracing::debug;

use crate::{
  failures::Failures,
  project::{FontConfigs, FontData, FontMap, FontType},
  publication::FontMetrics,
  typeset::font::{
    FontLoadError, FontRefs, build_font_metrics,
    face_config::{FontFaceConfigs, build_face_configs},
    parse_fonts,
    shaper::{self, Buffer, HarfRustShapers, ShaperError, ShapingFonts},
    validation::{self, FontValidationError, FontWarning},
  },
};

/// [`FontSystem::load`] のエラー 1 件。
///
/// **1 フォントぶんの違反 1 件**を表し、複数フォントの違反は `Failures<FontSystemError>` の
/// 別要素になる。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum FontSystemError {
  /// フォント解析・メトリクス取得の失敗
  #[error(transparent)]
  #[diagnostic(transparent)]
  Load(#[from] FontLoadError),
  /// フォント設定検証の失敗
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] FontValidationError),
  /// シェーパー初期化の失敗
  #[error(transparent)]
  #[diagnostic(transparent)]
  Shaper(#[from] ShaperError),
}

/// シェイプ・メトリクス取得と描画用の設定を提供する、検証済みのフォント資源。
///
/// シェーパーはフォントを所有し、バイト列だけを `FontData` と共有する。
pub(in crate::typeset) struct FontSystem {
  /// 19 種別ぶんのシェーパー
  shapers: HarfRustShapers,
  /// 基本メトリクス
  metrics: FontMap<FontMetrics>,
  /// シェーパーと同じ設定から確定した描画用のフェース設定
  face_configs: FontFaceConfigs,
}

impl FontSystem {
  /// 読み込み済み `FontData` から、検証済みのフォント資源一式を構築する。
  ///
  /// 組の第 2 要素は検証で見つかった警告（[`FontWarning`]）。検証・シェーパー初期化が失敗しても警告は返す。
  /// 解析・メトリクス取得で失敗したときは検証に進んでいないので、警告は空。
  ///
  /// # Errors
  ///
  /// フォント解析・メトリクス取得・設定検証・シェーパー初期化のいずれかに失敗した場合に、組の第 1 要素が、その段で
  /// 見つかった違反を [`FontSystemError`] の非空集合として持つ。
  pub(crate) fn load(
    configs: &FontConfigs,
    font_data: &FontData,
  ) -> (Result<Self, Failures<FontSystemError>>, Vec<FontWarning>) {
    let (font_refs, shaping_fonts, metrics) = match parse_and_measure(configs, font_data) {
      Ok(built) => built,
      Err(failures) => return (Err(failures), Vec::new()),
    };

    let stage_start = Instant::now();
    let (validated, warnings) = validation::validate_fonts(configs, &font_refs, &shaping_fonts);
    if let Err(failures) = validated {
      return (Err(failures.map(Into::into)), warnings);
    }
    debug!(warning_count = warnings.len(), elapsed = ?stage_start.elapsed(), "全種別のフォントを検証");

    let shapers = match shaper::build_harfrust_shapers(configs, shaping_fonts) {
      Ok(shapers) => shapers,
      Err(failures) => return (Err(failures.map(Into::into)), warnings),
    };
    debug!("シェーパーを初期化");

    return (
      Ok(Self {
        shapers,
        metrics,
        face_configs: build_face_configs(configs),
      }),
      warnings,
    );
  }

  /// 指定フォント種別でテキストをシェイプし、結果のグリフ列を `buffer` に残す。
  pub(crate) fn shape(&self, font_type: FontType, buffer: &mut Buffer, text: &str, point_size: f32) {
    self.shapers[font_type].shape(buffer, text, point_size);
  }

  /// 指定フォント種別の基本メトリクスを返す。
  #[must_use]
  pub(crate) fn metrics(&self, font_type: FontType) -> FontMetrics { return self.metrics[font_type]; }

  /// 数式フォントの MATH テーブルの、フォント全体の定数（値はフォント単位）。
  #[must_use]
  pub(crate) fn math_constants(&self) -> MathConstants<'_> {
    return self.shapers[FontType::Math].font().tables().math().and_then(|math| return math.math_constants()).expect(
      "load の検証（validation::check_math_table）が、このシェイピング用フォントのテーブルから MATH と MathConstants を読めることを確認済み",
    );
  }

  /// シェーパーと同じフェース・バリエーション軸の描画用設定。
  #[must_use]
  pub(crate) fn face_configs(&self) -> &FontFaceConfigs { return &self.face_configs; }
}

/// フォントの解析とメトリクスの取得を行う（検証の前の 2 段）。
///
/// # Errors
///
/// いずれかのフォントを解析できない、またはメトリクスを取得できない場合に、その段の違反を全件返す。
fn parse_and_measure<'a>(
  configs: &FontConfigs,
  font_data: &'a FontData,
) -> Result<(FontRefs<'a>, ShapingFonts, FontMap<FontMetrics>), Failures<FontSystemError>> {
  let (font_refs, shaping_fonts) =
    parse_fonts(configs, font_data).map_err(|failures| return failures.map(Into::into))?;
  let metrics = build_font_metrics(&font_refs).map_err(|failures| return failures.map(Into::into))?;
  return Ok((font_refs, shaping_fonts, metrics));
}
