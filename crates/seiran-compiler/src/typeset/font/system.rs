//! フォント資源の構築順序を隠蔽する窓口モジュール
//!
//! `FontData` → `FontRefs` → 検証 → `ShaperDatas` / `ShaperInstances` → `HarfRustShapers` という
//! 構築順序と寿命関係をここに閉じ込め、呼び出し側には [`FontResources::load`] と
//! [`FontResources::system`] の 2 段呼び出しだけを公開する。

use std::time::Instant;

use miette::Diagnostic;
use thiserror::Error;
use tracing::debug;

use crate::{
  failures::Failures,
  project::{FontConfigs, FontData, FontType},
  typeset::font::{
    FontLoadError, FontMetric, FontMetrics, FontRefs, build_font_metrics, build_font_refs,
    face_config::{FontFaceConfigs, build_face_configs},
    shaper::{
      HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperError, ShaperInstances,
      ShaperInstancesExt, UnicodeBuffer,
    },
    validate_font::{self, FontValidationFailure, FontWarning},
  },
};

/// [`FontResources::load`] / [`FontResources::system`] のエラー 1 件。
///
/// 既存の [`FontLoadError`] / [`FontValidationFailure`] / [`ShaperError`] を運ぶ薄いラッパーで、
/// `transparent` でメッセージ・code・help・label・related をすべて内側の leaf へ委譲する。
/// **1 フォントぶんの違反 1 件**を表し、複数フォントの違反は `Failures<FontSystemError>` の
/// 別要素になる（related 付きの 1 診断へ束ねると、`FontType::ALL` 順のフラットな leaf 列に
/// ならない）。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum FontSystemError {
  /// フォント解析・メトリクス取得の失敗（`build_font_refs` / `build_font_metrics` に由来）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Load(#[from] FontLoadError),
  /// フォント設定検証の失敗（[`validate_font::validate_fonts`] に由来）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] FontValidationFailure),
  /// シェーパー初期化の失敗（[`HarfRustShapers::new`] に由来）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Shaper(#[from] ShaperError),
}

/// `FontData` を借用し、フォント資源の「所有される」部分をまとめる。
///
/// フィールドは互いを借用しない（`font_refs` だけが外部の `FontData` を借用する）。
/// `HarfRustShapers` は `FontRefs` と `ShaperDatas`/`ShaperInstances`（本来なら兄弟フィールド）を
/// 両方借用し続ける実装のため、1 つの構造体が両方を所有すると自己参照構造体になってしまう。
/// これを避けるため、シェーパー本体は [`FontSystem`] という別の薄いビューへ分離している。
pub(crate) struct FontResources<'a> {
  /// `load` に渡された設定（`system` が同じ設定でシェーパーを構築するために保持する）
  configs: &'a FontConfigs,
  /// 解析済み OpenType フォント参照（`FontData` を借用）
  font_refs: FontRefs<'a>,
  /// シェイピング用の解析データ（所有）
  shaper_datas: ShaperDatas,
  /// バリエーション軸インスタンス（所有）
  shaper_instances: ShaperInstances,
  /// 基本メトリクス（所有）
  metrics: FontMetrics,
}

impl<'a> FontResources<'a> {
  /// 読み込み済み `FontData` から、検証済みのフォント資源一式を構築する。
  ///
  /// 構築順序は `FontRefs → FontMetrics → 検証 → ShaperDatas → ShaperInstances`
  /// （現行の `compiler.rs` / `compile.rs` の実行順序と同一。診断の出方を変えないため厳守する。
  /// issue #278 タイトルの表記順序とは異なる点に注意）。
  ///
  /// 各段の中ではフォントを独立に検査して違反を `FontType::ALL` 順に全件集めるが、**段の間**は
  /// 早期 return する — 解析できなかったフォントのメトリクスは取得できず、メトリクスの無い
  /// フォントは検証できないという依存があるため（#376 の「後続の入力を構築できないなら集約しない」）。
  ///
  /// 検証で見つかった警告（[`FontWarning`]）は資源ではなくコンパイルの副産物なので、
  /// この構造体には持たせず戻り値の第 2 要素として外へ出す。
  ///
  /// # Errors
  ///
  /// フォント解析・メトリクス取得・設定検証のいずれかに失敗した場合に、その段で見つかった
  /// 違反を [`FontSystemError`] の非空集合として返す。
  pub(crate) fn load(
    configs: &'a FontConfigs,
    font_data: &'a FontData,
  ) -> Result<(Self, Vec<FontWarning>), Failures<FontSystemError>> {
    let font_refs = build_font_refs(configs, font_data).map_err(|failures| return failures.map(Into::into))?;
    let metrics = build_font_metrics(&font_refs).map_err(|failures| return failures.map(Into::into))?;

    let stage_start = Instant::now();
    let warnings =
      validate_font::validate_fonts(configs, &font_refs).map_err(|failures| return failures.map(Into::into))?;
    debug!(elapsed_ms = elapsed_ms(stage_start), warning_count = warnings.len(), "フォントの検証が完了しました");

    let shaper_datas = ShaperDatas::new(&font_refs);
    let shaper_instances = ShaperInstances::new(configs, &font_refs);
    return Ok((
      Self {
        configs,
        font_refs,
        shaper_datas,
        shaper_instances,
        metrics,
      },
      warnings,
    ));
  }

  /// `Publication` の描画資源を組み立てるための `FontMetrics` アクセサ。
  #[must_use]
  pub(crate) fn metrics(&self) -> &FontMetrics { return &self.metrics; }

  /// `Publication` の描画資源を組み立てるための [`FontFaceConfigs`] アクセサ。
  #[must_use]
  pub(crate) fn face_configs(&self) -> FontFaceConfigs { return build_face_configs(self.configs); }

  /// シェーパー一式を構築し、シェイプ操作だけを公開する [`FontSystem`] を返す。
  ///
  /// `load` に渡されたのと同じ設定（`self.configs`）を使う — 呼び出し側が別の設定を渡して
  /// フォント参照・バリエーション軸と食い違うシェーパーを組んでしまう余地をなくすため、
  /// 引数では受け取らない。
  ///
  /// 戻り値の [`FontSystem`] は `self` を借用するだけの寿命を持ち、`FontData` への寿命 `'a` とは
  /// 独立する（`'a` に固定すると呼び出し側で `FontResources` の借用が不必要に長引く）。
  ///
  /// # Errors
  ///
  /// 言語タグの解析に失敗した場合に [`FontSystemError`] の非空集合を返す。
  pub(crate) fn system(&self) -> Result<FontSystem<'_>, Failures<FontSystemError>> {
    let shapers = HarfRustShapers::new(self.configs, &self.font_refs, &self.shaper_datas, &self.shaper_instances)
      .map_err(|failures| return failures.map(Into::into))?;
    debug!("シェーパーの初期化が完了しました");
    return Ok(FontSystem {
      shapers,
      metrics: &self.metrics,
    });
  }
}

/// シェイプ・メトリクス取得だけを公開するビュー。
///
/// 呼び出し側は `FontRefs`/`ShaperDatas`/`ShaperInstances`/`HarfRustShapers` の構築順序・寿命関係を
/// 一切知らない。
pub(crate) struct FontSystem<'a> {
  /// 19 種別ぶんのシェーパー
  shapers: HarfRustShapers<'a>,
  /// フォントメトリクス（[`FontResources`] を借用）
  metrics: &'a FontMetrics,
}

impl FontSystem<'_> {
  /// 指定フォント種別でテキストをシェイプする。
  ///
  /// `buffer` は返り値に `clear()` を呼ぶことで再利用できる。
  #[must_use]
  pub(crate) fn shape(
    &self,
    font_type: FontType,
    buffer: UnicodeBuffer,
    text: &str,
    point_size: f32,
  ) -> harfrust::GlyphBuffer {
    return self.shapers.get(font_type).shape(buffer, text, point_size);
  }

  /// 指定フォント種別の基本メトリクスを返す。
  #[must_use]
  pub(crate) fn metric(&self, font_type: FontType) -> FontMetric { return *self.metrics.get(font_type); }
}

/// ステージ開始時刻からの経過ミリ秒を返す（DEBUG ログの `elapsed_ms` 用）。
#[expect(clippy::cast_possible_truncation, reason = "経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはない")]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }
