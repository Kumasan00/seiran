//! 設定ファイルの `sources` から描画直前の `Publication` を構築するコンパイル facade。
//!
//! PDF バイト列の生成は `seiran-pdf`、ファイルへの保存は CLI の責務で、この module は
//! どちらも行わない。

mod compile_failure;
mod dependency_manifest;
#[cfg(test)]
mod diagnostics;
#[cfg(test)]
mod dump;
#[cfg(test)]
mod golden;
mod input;
#[cfg(test)]
mod project_source_equivalence;
mod source_diagnostic;
#[cfg(test)]
mod test_support;
mod warnings;

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
  document::{HirDocument, HirSource},
  failures, frontend,
  phase::Phase,
  project::{PathResolver, ProjectPath, ProjectSource, SourceSet, config::ConfigWarning},
  publication::Publication,
  semantics::{self, AnalyzeError, SemanticDocument, SemanticError},
  typeset::{self, TypesetOutput},
};

/// 型消去済みの診断 1 件（error・warning 共通の保持形）。
type BoxedDiagnostic = Box<dyn miette::Diagnostic + Send + Sync + 'static>;

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
  /// 出力 PDF の保存先（組版の成果ではなく検証済み設定から決まる値）
  pub pdf_path: PathBuf,
}

/// `source`、`root`（設定ファイルパス）、`base_dir`（相対パスの解決基準）から
/// PDF 直前の `Publication` までを 1 回で作る。
///
/// `base_dir` は呼び出し元が実行環境に応じて明示し、本関数はカレントディレクトリを取得しない。
/// 相対 `root` は読み込みの前に `base_dir` を基準に解決するため、`Compilation.dependencies.config_path`
/// と診断が示す設定ファイルパスは解決後の値になる。
///
/// # Errors
///
/// 設定・ソース・文献・フォント・画像の読込、パース、意味解決、組版のいずれかに失敗した場合、
/// 1 件以上の error diagnostic を持つ [`CompileFailure`] を返す。失敗するまでに確定した警告は
/// [`CompileFailure::warnings`] に入力の論理順で入っている。
pub fn compile<S: ProjectSource>(
  source: &S,
  root: &ProjectPath,
  base_dir: &Path,
) -> Result<Compilation, CompileFailure> {
  let phase = Phase::enter(info_span!("compile"));
  let build_start = Instant::now();

  let mut warnings = Warnings::default();
  let Compiled {
    publication,
    dependencies,
    pdf_path,
  } = match run_phases(source, root, base_dir, &mut warnings) {
    Ok(compiled) => compiled,
    Err(failure) => return Err(failure.with_warnings(warnings)),
  };

  let statistics = BuildStatistics {
    page_count: publication.pages().len(),
    // `as_millis` は u128 を返すが、経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはないので飽和で足りる
    total_elapsed_ms: u64::try_from(build_start.elapsed().as_millis()).unwrap_or(u64::MAX),
  };
  info!(page_count = statistics.page_count, warning_count = warnings.iter().count(), "文書をコンパイル");
  phase.succeed();

  return Ok(Compilation {
    publication,
    dependencies,
    warnings,
    statistics,
    pdf_path,
  });
}

/// [`run_phases`] の成果のうち、警告と統計を除いた部分。
struct Compiled {
  /// 描画直前の確定済み出版物
  publication: Publication,
  /// 読み取った外部資源のパス一覧
  dependencies: DependencyManifest,
  /// 出力 PDF の保存先
  pdf_path: PathBuf,
}

/// 入力読込から組版までの phase を順に実行し、各段が返した警告を段の実行順で `warnings` へ積む。
///
/// 警告は段が失敗しても捨てない — 段が返した警告は、その段や後段が失敗してもその時点で確定しているため。
/// `warnings` へ積むのは各段の戻り値だけで、段の内側から直接積む経路は作らない。
///
/// # Errors
///
/// いずれかの phase が失敗した場合に、その phase の失敗を返す。
fn run_phases(
  source: &dyn ProjectSource,
  root: &ProjectPath,
  base_dir: &Path,
  warnings: &mut Warnings,
) -> Result<Compiled, CompileFailure> {
  let (resolver, root) = resolve_root(root, base_dir);
  let (inputs, config_warnings) = load_inputs(source, &root, &resolver);
  warnings.extend(config_warnings);
  let inputs = inputs?;
  let semantic_document = analyze_document(source, &inputs, &resolver)?;
  let (typeset_output, typeset_warnings) = typeset::compose(
    source,
    inputs.config(),
    inputs.style(),
    inputs.geometry(),
    inputs.font_data(),
    &semantic_document,
  );
  warnings.extend(typeset_warnings);
  let TypesetOutput {
    publication,
    image_paths,
  } = typeset_output.map_err(CompileFailure::from)?;

  return Ok(Compiled {
    publication,
    dependencies: DependencyManifest::collect(&root, &inputs, &image_paths),
    pdf_path: inputs.config().output.pdf_path(),
  });
}

/// `base_dir` から入力パスの resolver を 1 回だけ構築し、`root`（設定ファイルパス）を同じ規則で解決する。
fn resolve_root(root: &ProjectPath, base_dir: &Path) -> (PathResolver, ProjectPath) {
  let resolver = PathResolver::new(base_dir);
  let root = resolver.resolve(root);
  return (resolver, root);
}

/// 入力読込 phase を実行する。
///
/// 戻り値は読込の成否と config の警告の組（警告は失敗しても返る）。
///
/// # Errors
///
/// 設定・スタイル・文献・フォント・ソースの読込または検証に失敗した場合に、組の第 1 要素がエラーになる。
fn load_inputs(
  source: &dyn ProjectSource,
  root: &ProjectPath,
  resolver: &PathResolver,
) -> (Result<CompilationInputs, CompileFailure>, Vec<ConfigWarning>) {
  let phase = Phase::enter(info_span!("input"));
  let (inputs, config_warnings) = input::load(source, root, resolver);
  if inputs.is_ok() {
    info!(config_path = %root, "入力を読込");
    phase.succeed();
  }
  return (inputs.map_err(CompileFailure::from), config_warnings);
}

/// 検証済み入力から意味解析済み文書までの 2 phase（frontend / semantics）を実行する。
///
/// # Errors
///
/// パース・意味解析のいずれかに失敗した場合にエラーを返す。
fn analyze_document(
  source: &dyn ProjectSource,
  inputs: &CompilationInputs,
  resolver: &PathResolver,
) -> Result<SemanticDocument, CompileFailure> {
  let document = {
    let phase = Phase::enter(info_span!("frontend"));
    let document = parse_project(inputs, resolver)?;
    info!(
      source_count = document.groups().len(),
      node_count = document.groups().iter().map(|group| return group.nodes.len()).sum::<usize>(),
      "ソースを構文解析"
    );
    phase.succeed();
    document
  };

  let semantic_document = {
    let phase = Phase::enter(info_span!("semantics"));
    let semantic_document = semantics::analyze(source, document, inputs.references(), inputs.style())
      .map_err(|error| return attribute_analyze_error(error, inputs.sources()))?;
    info!(heading_count = semantic_document.heading_count(), "文書を意味解析");
    phase.succeed();
    semantic_document
  };
  return Ok(semantic_document);
}

/// 全ソースをパースし、1 つの文書木（HIR）へまとめる。
///
/// # Errors
///
/// パース・評価エラーが集約して返る場合にエラーを返す。
fn parse_project(inputs: &CompilationInputs, resolver: &PathResolver) -> Result<HirDocument, CompileFailure> {
  let document = HirDocument::assemble(parse_all_sources(inputs.sources(), resolver)?);
  return Ok(document);
}

/// 全ソースをパースし、パース・評価エラーを集約する。
///
/// エラーは宣言順に並べ、先頭（最初に失敗したソースの leaf 診断）を主診断にする。
fn parse_all_sources(sources: &SourceSet, resolver: &PathResolver) -> Result<Vec<HirSource>, CompileFailure> {
  let results = sources
    .iter()
    .map(|(source_id, entry)| {
      return frontend::parse_source(&entry.content, source_id, resolver)
        .map_err(|error| return SourceDiagnostic::attach(sources, source_id, error));
    })
    .collect();
  return failures::collect_in_input_order(results).map_err(CompileFailure::from);
}

/// `semantics::analyze` のエラーへソース本文を添え、表示可能な診断の集合にする。
///
/// CSL 由来（`CitationStyle`）はそれ自身が leaf 診断なのでそのまま運ぶ。
fn attribute_analyze_error(error: AnalyzeError, sources: &SourceSet) -> CompileFailure {
  return match error {
    AnalyzeError::CitationStyle(error) => CompileFailure::single(error),
    AnalyzeError::Analyze(errors) => {
      CompileFailure::from(errors.map(|error| return attach_semantic_error(sources, error)))
    },
  };
}

/// 意味解析の診断 1 件へ、帰属するソースの本文と、別ソースにある関連位置（そのソースの本文付き）を添える。
///
/// 関連位置を持つかどうか・その文言は semantics が決め（`SemanticError::first_definition_elsewhere`）、
/// ここは確定 ID で本文を引いて添えるだけ。
fn attach_semantic_error(sources: &SourceSet, error: SemanticError) -> SourceDiagnostic<SemanticError> {
  let elsewhere = error.first_definition_elsewhere();
  let diagnostic = SourceDiagnostic::attach(sources, error.source_id(), error);
  return match elsewhere {
    Some(note) => diagnostic.with_related_in(sources, note.source_id(), note),
    None => diagnostic,
  };
}
