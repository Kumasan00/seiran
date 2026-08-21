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

use tracing::debug;

use super::{elapsed_ms, error::CompileError};
use crate::{
  failures::Failures,
  project::{
    FontData, ProjectSource, SourceSet,
    config::{ConfigWarning, ProjectConfig},
  },
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
  /// 設定の読込で見つかった警告（`sources` の宣言順）
  config_warnings: Vec<ConfigWarning>,
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

  /// 設定の読込で見つかった警告を宣言順に返す。
  pub(super) fn config_warnings(&self) -> &[ConfigWarning] { return &self.config_warnings; }

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
  #[cfg(test)]
  #[allow(
    clippy::result_large_err,
    reason = "位置付き診断のため `NamedSource` を同梱する Err を返す（入力読込は 1 回だけでサイズは最適化対象ではない）"
  )]
  pub(super) fn from_parts(
    source: &dyn ProjectSource,
    config: ProjectConfig,
    style: Style,
    references: Arc<References>,
    font_data: FontData,
  ) -> Result<Self, Failures<CompileError>> {
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
      config_warnings: Vec::new(),
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
///
/// **段の間は早期 return する** — config が読めなければ style path が決まらず、style が無ければ
/// 横断検証ができない、というように後段の入力を構築できないため（#376 の集約規則）。段の中で
/// 独立に検査できるもの（複数フォントパス・複数ソース）は全件を集約する。
#[allow(
  clippy::result_large_err,
  reason = "位置付き診断のため `NamedSource` を同梱する Err を返す（入力読込は 1 回だけでサイズは最適化対象ではない）"
)]
pub(super) fn load(
  source: &dyn ProjectSource,
  config_path: &Path,
  base_dir: &Path,
) -> Result<CompilationInputs, Failures<CompileError>> {
  let (config, config_warnings) = crate::project::config::load(source, config_path, base_dir).map_err(lift)?;
  let style = crate::style::load(source, config.style_path.as_deref(), base_dir).map_err(lift)?;
  crate::typeset::validate_layout(&config, &style).map_err(lift)?;
  let references = Arc::new(read_references(source, config.references_path.as_deref()).map_err(single)?);

  let stage_start = Instant::now();
  let font_data =
    FontData::load(source, &config.font_configs).map_err(|failures| return failures.map(CompileError::from))?;
  debug!(elapsed_ms = elapsed_ms(stage_start), "フォントファイルの読み込みが完了しました");

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
    config_warnings,
  });
}

/// 段まるごとの失敗（後続の入力を構築できないもの）を 1 件の非空集合へ包む。
fn single<E: Into<CompileError>>(error: E) -> Failures<CompileError> { return Failures::single(error.into()); }

/// 段が集めた非空集合を、そのまま `CompileError` の非空集合へ持ち上げる。
fn lift<E: Into<CompileError>>(failures: Failures<E>) -> Failures<CompileError> { return failures.map(Into::into); }

/// `config.sources` を読み込み、失敗を位置付き診断へ組み替える。
///
/// `project::SourceSet` はどのパスがどう失敗したかだけを返し、役割（テキストファイル）と
/// パスを含む leaf diagnostic を組み立てるのはここ。seam の `SourceReadError` は
/// `Diagnostic` を実装しない低水準 cause なので、そのまま `#[source]` に載せても
/// 入れ子の診断ブロックにはならない（#377）。
#[allow(
  clippy::result_large_err,
  reason = "位置付き診断のため `NamedSource` を同梱する Err を返す（入力読込は 1 回だけでサイズは最適化対象ではない）"
)]
fn read_sources(source: &dyn ProjectSource, sources: &[PathBuf]) -> Result<SourceSet, Failures<CompileError>> {
  return SourceSet::read(source, sources).map_err(|failures| {
    return failures.map(|error| {
      return CompileError::ReadTextFile {
        path: error.path,
        source: error.source,
      };
    });
  });
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{CompileError, read_sources};
  use crate::project::{MemoryProjectSource, SourceReadError};

  /// `project::SourceSet` の素のエラーを、移設前と同じ位置付き診断へ組み替えることを固定する。
  ///
  /// `code` と役割・パスを含むメッセージの組み立ては `project` ではなくここの責務なので、
  /// `SourceSet::read` 側のテストではこの層を通らない（#351）。
  #[test]
  fn read_sources_maps_missing_file_to_read_text_file_diagnostic() {
    // Arrange — 存在するソースと存在しないソースを混ぜる
    let source = MemoryProjectSource::new().with_text("/project/a.sei", "content-a");
    let sources = vec![
      PathBuf::from("/project/a.sei"),
      PathBuf::from("/project/missing.sei"),
    ];

    // Act
    let result = read_sources(&source, &sources);

    // Assert
    let Err(failures) = result else {
      panic!("ReadTextFile を期待");
    };
    let CompileError::ReadTextFile {
      path,
      source: read_error,
    } = failures.first()
    else {
      panic!("ReadTextFile を期待");
    };
    assert_eq!(path, "/project/missing.sei");
    assert!(
      matches!(read_error, SourceReadError::NotFound),
      "seam のエラーは cause として保たれるはず: {read_error:?}"
    );
  }

  #[test]
  fn read_sources_reports_every_missing_file_in_declaration_order() {
    // Arrange — 2 つの欠落を宣言順とは逆のパス名で並べる（宣言順で報告されることを見る）
    let source = MemoryProjectSource::new();
    let sources = vec![
      PathBuf::from("/project/z-missing.sei"),
      PathBuf::from("/project/a-missing.sei"),
    ];

    // Act
    let Err(failures) = read_sources(&source, &sources) else {
      panic!("2 件とも失敗するはず");
    };

    // Assert — パス名の辞書順ではなく config.sources の宣言順
    let paths: Vec<&str> = failures
      .iter()
      .map(|error| {
        let CompileError::ReadTextFile { path, .. } = error else {
          panic!("ReadTextFile を期待");
        };
        return path.as_str();
      })
      .collect();
    assert_eq!(paths, vec!["/project/z-missing.sei", "/project/a-missing.sei"]);
  }
}
