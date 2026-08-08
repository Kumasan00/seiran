//! 入力読込の唯一の入口 [`load`] と、その成果物 [`CompilationInputs`] / [`OutputPlan`]
//!
//! config.toml → style.toml → 横断検証 → 文献 → フォント → ソース という順序は、
//! 前段の結果が次段の入力になる（style / references のパスは config.toml が持ち、
//! 横断検証は config × style の両方を要求する）。この順序とエラー集約を知るのはこの module だけで、
//! 呼び出し元（`compile`）は [`load`] を 1 回呼ぶだけになる（#351）。
//!
//! CSL スタイル・ロケールはここでは読まない — 引用箇所が 1 つも無ければ `.csl` を読まない
//! という遅延は `semantics::analyze` の内側に閉じている。

use std::{
  path::{Path, PathBuf},
  sync::Arc,
  time::Instant,
};

use tracing::info;

use super::{elapsed_ms, error::CompileError};
use crate::{
  font::{FontData, FontDataExt},
  project::{ProjectSource, SourceSet, config::ProjectConfig},
  semantics::{References, read_references},
  style::Style,
};

/// 読込・個別検証・横断検証をすべて通った入力。
///
/// フィールドは非公開で、production の構築経路は [`load`] だけ。「検証を通っていない値が
/// 後段へ流れない」ことを型で保証するため、外から組み立てられるコンストラクタを持たない
/// （受け入れ条件、#351）。画像はパース後にパスが分かるため含めない。
pub(super) struct CompilationInputs {
  /// 検証済みの設定（用紙・余白・`sources`・`font_configs` 等）
  config: ProjectConfig,
  /// 検証済みのスタイル
  style: Style,
  /// `\cite` の CSL 整形に使う文献データ。複数の入力（golden テスト等）で使い回せるよう
  /// `Arc` で共有する
  references: Arc<References>,
  /// 読込済みの全フォントバイナリ
  font_data: FontData,
  /// ソースファイルごとの読込済みテキスト（`SourceId` で引ける）
  sources: SourceSet,
  /// 保存先など、書き込みを行う呼び出し側だけが使う出力情報
  output: OutputPlan,
}

impl CompilationInputs {
  /// 検証済みの設定を返す。
  pub(super) fn config(&self) -> &ProjectConfig { return &self.config; }

  /// 検証済みのスタイルを返す。
  pub(super) fn style(&self) -> &Style { return &self.style; }

  /// 文献データを返す。
  pub(super) fn references(&self) -> &Arc<References> { return &self.references; }

  /// 読込済みフォントバイナリを返す。
  pub(super) fn font_data(&self) -> &FontData { return &self.font_data; }

  /// 読込済みソース集合を返す。
  pub(super) fn sources(&self) -> &SourceSet { return &self.sources; }

  /// 出力先情報を返す。
  pub(super) fn output(&self) -> &OutputPlan { return &self.output; }

  /// 型付きの設定・スタイルから入力を組み立てるテスト専用のコンストラクタ。
  ///
  /// golden の style 差分テスト（`layout_dump_changes_with_line_height` ほか）と
  /// `build_pages_with_source` は、TOML を経ずにメモリ上で `Style` を書き換えて組み直す。
  /// そのため [`load`] とは別に入口が要る。production からは呼べないよう `#[cfg(test)]` に
  /// 閉じてあり、受け入れ条件「検証済みの値だけが入る」は production 経路で保たれる。
  ///
  /// # Errors
  ///
  /// いずれかのソースファイルの読込に失敗した場合にエラーを返す。
  // NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
  #[cfg(test)]
  #[allow(clippy::result_large_err)]
  pub(super) fn from_parts(
    source: &dyn ProjectSource,
    config: ProjectConfig,
    style: Style,
    references: Arc<References>,
    font_data: FontData,
  ) -> Result<Self, CompileError> {
    let sources = read_sources(source, &config.sources)?;
    let output = OutputPlan {
      pdf_path: config.output.pdf_path(),
    };
    return Ok(CompilationInputs {
      config,
      style,
      references,
      font_data,
      sources,
      output,
    });
  }
}

/// 保存先など、書き込みを行う呼び出し側だけが使う出力情報。
#[derive(Debug, Clone)]
pub struct OutputPlan {
  /// 出力 PDF のパス
  pub pdf_path: PathBuf,
}

/// 設定・スタイル・文献・フォント・ソースを読み込み、検証済みの入力を組み立てる。
///
/// `source` は呼び出し元が 1 回だけ構築したものを受け取り、ここでは構築しない。
///
/// # Errors
///
/// 設定・スタイルの読込または検証、両者の横断検証、文献・フォント・ソースの読込のいずれかに
/// 失敗した場合にエラーを返す。
pub(super) fn load(
  source: &dyn ProjectSource,
  config_path: &Path,
  base_dir: &Path,
) -> miette::Result<CompilationInputs> {
  let config = crate::project::config::load(source, config_path, base_dir)?;
  let style = crate::style::load(source, config.style_path.as_deref(), base_dir)?;
  crate::typeset::validate_layout(&config, &style).map_err(|source| return CompileError::Layout { source })?;
  let references = Arc::new(read_references(source, config.references_path.as_deref())?);

  let stage_start = Instant::now();
  let font_data = FontData::new(source, &config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  let sources = read_sources(source, &config.sources)?;
  let output = OutputPlan {
    pdf_path: config.output.pdf_path(),
  };

  return Ok(CompilationInputs {
    config,
    style,
    references,
    font_data,
    sources,
    output,
  });
}

/// `config.sources` を読み込み、失敗を位置付き診断へ組み替える。
///
/// `project::SourceSet` はどのパスがどう失敗したかだけを返し、診断の `code` と
/// `into_io()` による平坦化はここが担う（seam のエラーをそのまま `#[source]` に載せると
/// miette が入れ子の診断ブロックを足し、表示が変わってしまうため）。
// NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
#[allow(clippy::result_large_err)]
fn read_sources(source: &dyn ProjectSource, sources: &[PathBuf]) -> Result<SourceSet, CompileError> {
  return SourceSet::read(source, sources).map_err(|error| {
    return CompileError::ReadTextFile {
      path: error.path,
      source: error.source.into_io(),
    };
  });
}
