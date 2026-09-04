//! 設定ファイルの `sources` から描画直前の `Publication` を構築するコンパイル facade。
//!
//! PDF バイト列の生成は `seiran-pdf`、ファイルへの保存は CLI の責務で、この module は
//! どちらも行わない。

use crate::{
  document::{HirDocument, HirSource},
  frontend,
  project::{ProjectPath, ProjectSource},
  semantics, typeset,
};

mod compile_failure;
mod dependency_manifest;
mod input;
mod source_diagnostic;
mod warnings;

#[cfg(test)]
mod diagnostics;
#[cfg(test)]
mod dump;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod project_source_equivalence;
#[cfg(test)]
mod test_support;

use std::{
  path::{Path, PathBuf},
  time::Instant,
};

pub use compile_failure::CompileFailure;
pub use dependency_manifest::DependencyManifest;
use input::CompilationInputs;
use source_diagnostic::SourceDiagnostic;
use tracing::{info, info_span};
pub use warnings::Warnings;

use crate::{
  project::SourceSet,
  publication::{self, Publication},
  semantics::AnalyzeError,
  typeset::{FontResources, FontWarning, LaidOutDocument, TypesetWarning},
};

/// コンパイル結果の統計情報。
#[derive(Debug, Clone, Copy)]
pub struct BuildStatistics {
  /// 確定ページ総数（前付け + 本文 + 後付け）
  pub page_count: usize,
  /// コンパイル全体の所要ミリ秒
  pub total_elapsed_ms: u64,
}

/// `compile` の結果。
///
/// 描画直前の `Publication` と、それに付随する情報（依存パス・警告・統計・出力先）を束ねる。
/// `Publication` 以外に組版の中間型は含まない。
#[derive(Debug)]
pub struct Compilation {
  /// 描画直前の確定済み出版物
  pub publication: Publication,
  /// `compile` が読み取った外部資源のパス一覧
  pub dependencies: DependencyManifest,
  /// 致命的ではない warning 診断（フォント・設定のうちユーザーが直せる非致命的な問題）
  pub warnings: Warnings,
  /// コンパイル結果の統計情報
  pub statistics: BuildStatistics,
  /// 出力 PDF の保存先。書き込みを行う呼び出し側だけが使う出力情報で、組版の成果ではなく
  /// 検証済み設定から決まる値
  pub pdf_path: PathBuf,
}

/// `source`、`root`（設定ファイルパス）、`base_dir`（相対パスの解決基準）から
/// PDF 直前の `Publication` までを 1 回で作る。
///
/// 言語処理・意味解決・組版を内部で順に実行する。呼び出し元は各段の中間型を知らない。
/// `base_dir` は呼び出し元が実行環境に応じて明示し、本関数はカレントディレクトリを取得しない。
/// 保存（PDF ファイルへの書き出し）は行わない — `Compilation.pdf_path` が指す先へ書き出すのは
/// 呼び出し元の責務とする。
///
/// # Errors
///
/// 設定・ソース・文献・フォント・画像の読込、パース、意味解決、組版のいずれかに失敗した場合、
/// 1 件以上の error diagnostic を持つ [`CompileFailure`] を返す。
pub fn compile<S: ProjectSource>(
  source: &S,
  root: &ProjectPath,
  base_dir: &Path,
) -> Result<Compilation, CompileFailure> {
  let _compile_span = info_span!("compile").entered();
  let build_start = Instant::now();

  let inputs = load_inputs(source, root, base_dir)?;
  let PipelineArtifacts {
    font_resources,
    laid_out,
    font_warnings,
    typeset_warnings,
  } = run_pipeline(source, &inputs)?;

  let dependencies = DependencyManifest::collect(root.as_ref(), &inputs, &laid_out.image_paths);
  let page_count = laid_out.pages.len();
  let publication = publication::build(inputs.config(), inputs.font_data(), &font_resources, laid_out);
  let warnings = collect_warnings(&inputs, font_warnings, typeset_warnings);
  let total_elapsed = build_start.elapsed();
  let statistics = BuildStatistics {
    page_count,
    // `as_millis` は u128 を返すが、経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはないので飽和で足りる
    total_elapsed_ms: u64::try_from(total_elapsed.as_millis()).unwrap_or(u64::MAX),
  };
  info!(
    page_count = statistics.page_count,
    warning_count = warnings.iter().count(),
    elapsed = ?total_elapsed,
    "文書をコンパイル"
  );

  return Ok(Compilation {
    publication,
    dependencies,
    warnings,
    statistics,
    pdf_path: inputs.config().output.pdf_path(),
  });
}

/// 入力読込 phase を実行する（production / test 共通）。
///
/// 読込順序とエラー集約は [`input::load`] が所有し、この関数が持つのは phase span と完了 event だけ。
///
/// # Errors
///
/// 設定・スタイル・文献・フォント・ソースの読込または検証に失敗した場合にエラーを返す。
fn load_inputs(
  source: &dyn ProjectSource,
  root: &ProjectPath,
  base_dir: &Path,
) -> Result<CompilationInputs, CompileFailure> {
  let _phase = info_span!("input").entered();
  let stage_start = Instant::now();
  let inputs = input::load(source, root.as_ref(), base_dir)?;
  info!(config_path = %root, elapsed = ?stage_start.elapsed(), "入力を読込");
  return Ok(inputs);
}

/// [`run_pipeline`] が確定させた組版までの成果物。
///
/// `Publication` への変換・依存パスの収集・統計の算出は行わず、`compile` がこの値を受け取って
/// 仕上げる。フォント資源は組版と描画資源の構築の両方が使うため、`LaidOutDocument` と一緒に運ぶ。
struct PipelineArtifacts<'a> {
  /// 組版と描画資源の構築が共有するフォント資源（`inputs` のフォントバイト列を借りる）
  font_resources: FontResources<'a>,
  /// 確定した組版結果
  laid_out: LaidOutDocument,
  /// フォント資源の構築で見つかった警告
  font_warnings: Vec<FontWarning>,
  /// 組版で見つかった警告
  typeset_warnings: Vec<TypesetWarning>,
}

/// 検証済み入力から組版までの 4 phase（frontend / semantics / font / typeset）を実行する
/// （production / test 共通）。
///
/// 各 phase の span・完了 event・診断への変換をここが所有し、`compile` と
/// `layout_project_for_test`（テスト専用）は同じ実装を通る。
///
/// # Errors
///
/// パース・意味解析・フォント資源の構築・組版のいずれかに失敗した場合にエラーを返す。
fn run_pipeline<'a>(
  source: &dyn ProjectSource,
  inputs: &'a CompilationInputs,
) -> Result<PipelineArtifacts<'a>, CompileFailure> {
  let document = {
    let _phase = info_span!("frontend").entered();
    let stage_start = Instant::now();
    let document = parse_project(inputs)?;
    info!(
      source_count = document.groups().len(),
      node_count = document.groups().iter().map(|group| return group.nodes.len()).sum::<usize>(),
      elapsed = ?stage_start.elapsed(),
      "ソースを構文解析"
    );
    document
  };

  let semantic_document = {
    let _phase = info_span!("semantics").entered();
    let stage_start = Instant::now();
    let semantic_document = semantics::analyze(source, document, inputs.references(), inputs.style())
      .map_err(|error| return attribute_analyze_error(error, inputs.sources()))?;
    info!(
      heading_count = semantic_document.headings().len(),
      elapsed = ?stage_start.elapsed(),
      "文書を意味解析"
    );
    semantic_document
  };

  let (font_resources, font_warnings) = {
    let _phase = info_span!("font").entered();
    let stage_start = Instant::now();
    let (font_resources, font_warnings) =
      FontResources::load(&inputs.config().font_configs, inputs.font_data()).map_err(CompileFailure::from)?;
    info!(
      warning_count = font_warnings.len(),
      elapsed = ?stage_start.elapsed(),
      "フォント資源を構築"
    );
    (font_resources, font_warnings)
  };

  let (laid_out, typeset_warnings) = {
    let _phase = info_span!("typeset").entered();
    let stage_start = Instant::now();
    let (laid_out, typeset_warnings) =
      typeset::layout(source, inputs.config(), inputs.style(), &font_resources, &semantic_document)
        .map_err(CompileFailure::from)?;
    info!(
      page_count = laid_out.pages.len(),
      warning_count = typeset_warnings.len(),
      elapsed = ?stage_start.elapsed(),
      "文書を組版"
    );
    (laid_out, typeset_warnings)
  };

  return Ok(PipelineArtifacts {
    font_resources,
    laid_out,
    font_warnings,
    typeset_warnings,
  });
}

/// 入力読込から組版までを production と同じ実装で通し、組版中間表現を取り出すテストヘルパ。
///
/// `Publication` へ変換すると失われる情報（anchor・索引語のページ帰属・脚注 fragment・
/// `PlacedBlock` の幾何）を検査するテストだけが使う。phase の処理は再実装せず
/// [`load_inputs`] と [`run_pipeline`] を呼ぶだけなので、`input::load` の横断検証を迂回できない。
///
/// # Errors
///
/// 入力読込または組版までのいずれかの phase が失敗した場合にエラーを返す。
#[cfg(test)]
fn layout_project_for_test(
  source: &dyn ProjectSource,
  root: &ProjectPath,
  base_dir: &Path,
) -> Result<LaidOutDocument, CompileFailure> {
  let inputs = load_inputs(source, root, base_dir)?;
  let artifacts = run_pipeline(source, &inputs)?;
  return Ok(artifacts.laid_out);
}

/// 成功した `Compilation` と一緒に返す warning を、**入力の論理順**で 1 つに束ねる。
///
/// 段の実行順（設定 → フォント → 組版）をそのまま表示順にする。段の中は各段が既に決定的な順序で
/// 集めている（設定は `sources` の宣言順、フォントは `FontType::ALL` 順、組版は物理ページの昇順）ので、
/// ここでの並べ替えは行わない。
fn collect_warnings(
  inputs: &CompilationInputs,
  font_warnings: Vec<FontWarning>,
  typeset_warnings: Vec<TypesetWarning>,
) -> Warnings {
  let mut warnings = Warnings::default();
  for warning in inputs.config_warnings() {
    warnings.push(warning.clone());
  }
  for warning in font_warnings {
    warnings.push(warning);
  }
  for warning in typeset_warnings {
    warnings.push(warning);
  }
  return warnings;
}

/// 全ソースをパースし、1 つの文書木（HIR）へまとめる。
///
/// 意味解析（ラベル・`\ref`・カウンタ・引用キー）と CSL 整形は `semantics::analyze` が、
/// 画像パスの収集は `typeset::layout` が担う。
///
/// # Errors
///
/// パース・評価エラーが集約して返る場合にエラーを返す。
fn parse_project(inputs: &CompilationInputs) -> Result<HirDocument, CompileFailure> {
  let document = HirDocument::assemble(parse_all_sources(inputs.sources())?);
  return Ok(document);
}

/// 全ソースをパースし、パース・評価エラーを集約する。
///
/// 戻り値はソースごとの HIR。プロジェクト全体の文書木への組み立ては呼び出し元が行う。
/// エラーは宣言順に並べ、先頭（最初に失敗したソースの leaf 診断）を主診断にする。
fn parse_all_sources(sources: &SourceSet) -> Result<Vec<HirSource>, CompileFailure> {
  let mut parsed: Vec<HirSource> = Vec::new();
  let mut parse_errors: Vec<Box<dyn miette::Diagnostic + Send + Sync + 'static>> = Vec::new();

  for (source_id, entry) in sources.iter() {
    match frontend::parse_source(&entry.content, source_id) {
      Ok(hir) => parsed.push(hir),
      Err(error) => parse_errors.push(Box::new(SourceDiagnostic::attach(sources, source_id, error))),
    }
  }

  if let Some(failure) = CompileFailure::from_diagnostics(parse_errors) {
    return Err(failure);
  }
  return Ok(parsed);
}

/// `semantics::analyze` のエラーへソース本文を添え、表示可能な診断の集合にする。
///
/// CSL 由来（`CitationStyle` / `CitationFormat`）はそれ自身が leaf 診断なのでそのまま運ぶ。
/// 意味解析由来はソースごとに分割済みなので、`SourceSet` から本文を引いて添えるだけでよい
/// （`SourceId` は `SourceSet::register` が発行した値をそのまま運んでいるため、ここでの参照は
/// 確定 ID による引き当てであり帰属元の推定ではない）。
fn attribute_analyze_error(error: AnalyzeError, sources: &SourceSet) -> CompileFailure {
  return match error {
    AnalyzeError::CitationStyle(error) => CompileFailure::single(error),
    AnalyzeError::CitationFormat(error) => CompileFailure::single(error),
    AnalyzeError::Analyze(failures) => {
      let (first, rest) = failures.into_parts();
      let mut failure = CompileFailure::single(SourceDiagnostic::attach(sources, first.source_id(), first));
      for error in rest {
        failure.push(SourceDiagnostic::attach(sources, error.source_id(), error));
      }
      failure
    },
  };
}
