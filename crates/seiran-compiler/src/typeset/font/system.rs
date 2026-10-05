//! フォント資源の構築順序を隠蔽する窓口モジュール
//!
//! `FontData` → `FontRefs` / `ShapingFonts` → `FontMap<FontMetrics>` → 検証 → `HarfRustShapers`
//! という構築順序と寿命関係をここに閉じ込め、呼び出し側には構築の入口として [`FontResources::load`] と
//! [`FontResources::system`] の 2 段呼び出しだけを公開する。

use std::time::Instant;

use miette::Diagnostic;
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

/// [`FontResources::load`] / [`FontResources::system`] のエラー 1 件。
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

/// フォント資源のうち、シェーパーを組む前に確定する部分をまとめる。
///
/// 借用するのは設定だけで、フォントのバイト列は `shaping_fonts` が `FontData` と共有して持つ。
pub(in crate::typeset) struct FontResources<'a> {
  /// `load` に渡された設定
  configs: &'a FontConfigs,
  /// 検証済みのシェイピング用フォント（バリエーション軸の位置を適用済み）
  shaping_fonts: ShapingFonts,
  /// 基本メトリクス（所有）
  metrics: FontMap<FontMetrics>,
}

impl<'a> FontResources<'a> {
  /// 読み込み済み `FontData` から、検証済みのフォント資源一式を構築する。
  ///
  /// 組の第 2 要素は検証で見つかった警告（[`FontWarning`]）。検証の違反で構築が失敗しても警告は返す。
  /// 解析・メトリクス取得で失敗したときは検証に進んでいないので、警告は空。
  ///
  /// # Errors
  ///
  /// フォント解析・メトリクス取得・設定検証のいずれかに失敗した場合に、組の第 1 要素が、その段で
  /// 見つかった違反を [`FontSystemError`] の非空集合として持つ。
  pub(crate) fn load(
    configs: &'a FontConfigs,
    font_data: &'a FontData,
  ) -> (Result<Self, Failures<FontSystemError>>, Vec<FontWarning>) {
    let (font_refs, shaping_fonts, metrics) = match parse_and_measure(configs, font_data) {
      Ok(built) => built,
      Err(failures) => return (Err(failures), Vec::new()),
    };

    let stage_start = Instant::now();
    let (validated, warnings) = validation::validate_fonts(configs, &font_refs);
    if let Err(failures) = validated {
      return (Err(failures.map(Into::into)), warnings);
    }
    debug!(warning_count = warnings.len(), elapsed = ?stage_start.elapsed(), "全種別のフォントを検証");

    return (
      Ok(Self {
        configs,
        shaping_fonts,
        metrics,
      }),
      warnings,
    );
  }

  /// 全フォント種別の基本メトリクス。
  #[must_use]
  pub(crate) fn metrics(&self) -> &FontMap<FontMetrics> { return &self.metrics; }

  /// [`FontFaceConfigs`] を構築して返す。
  #[must_use]
  pub(crate) fn face_configs(&self) -> FontFaceConfigs { return build_face_configs(self.configs); }

  /// シェーパー一式を構築し、シェイプ操作だけを公開する [`FontSystem`] を返す。
  ///
  /// `load` に渡されたのと同じ設定（`self.configs`）を使うので、シェイピング用フォント・バリエーション軸と
  /// 食い違うシェーパーは組めない。
  ///
  /// # Errors
  ///
  /// 言語タグの解析に失敗した場合に [`FontSystemError`] の非空集合を返す。
  pub(crate) fn system(&self) -> Result<FontSystem<'_>, Failures<FontSystemError>> {
    let shapers = shaper::build_harfrust_shapers(self.configs, &self.shaping_fonts)
      .map_err(|failures| return failures.map(Into::into))?;
    debug!("シェーパーを初期化");
    return Ok(FontSystem {
      shapers,
      metrics: &self.metrics,
    });
  }
}

/// フォントの解析とメトリクスの取得を行う（検証の前の 2 段）。
///
/// # Errors
///
/// いずれかのフォントを解析できない、またはメトリクスを取得できない場合に、その段の違反を全件返す。
fn parse_and_measure<'a>(
  configs: &'a FontConfigs,
  font_data: &'a FontData,
) -> Result<(FontRefs<'a>, ShapingFonts, FontMap<FontMetrics>), Failures<FontSystemError>> {
  let (font_refs, shaping_fonts) =
    parse_fonts(configs, font_data).map_err(|failures| return failures.map(Into::into))?;
  let metrics = build_font_metrics(&font_refs).map_err(|failures| return failures.map(Into::into))?;
  return Ok((font_refs, shaping_fonts, metrics));
}

/// シェイプ・メトリクス取得だけを公開するビュー。
pub(in crate::typeset) struct FontSystem<'a> {
  /// 19 種別ぶんのシェーパー
  shapers: HarfRustShapers<'a>,
  /// フォントメトリクス（[`FontResources`] を借用）
  metrics: &'a FontMap<FontMetrics>,
}

impl FontSystem<'_> {
  /// 指定フォント種別でテキストをシェイプし、結果のグリフ列を `buffer` に残す。
  pub(crate) fn shape(&self, font_type: FontType, buffer: &mut Buffer, text: &str, point_size: f32) {
    self.shapers[font_type].shape(buffer, text, point_size);
  }

  /// 指定フォント種別の基本メトリクスを返す。
  #[must_use]
  pub(crate) fn metrics(&self, font_type: FontType) -> FontMetrics { return self.metrics[font_type]; }
}
