//! 設定ファイルの `sources` から描画直前の `Publication` を構築するコンパイル facade。
//!
//! PDF バイト列の生成は `seiran-pdf`、ファイルへの保存は CLI の責務で、この module は
//! どちらも行わない。

use crate::{
  document::{HirDocument, HirSource},
  frontend,
  project::{PathResolver, ProjectPath, ProjectSource},
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

#[cfg(test)]
use crate::typeset::LaidOutDocument;
use crate::{
  project::{SourceSet, config::ConfigWarning},
  publication::Publication,
  semantics::{AnalyzeError, SemanticDocument},
  typeset::TypesetOutput,
};

/// 型消去済みの診断 1 件（error・warning 共通の保持形）。
///
/// [`miette::Report`] ではなく `Box<dyn Diagnostic>` にするのは、`Report` が `Diagnostic` を
/// 実装しない（miette 側の trait coherence の制約）ため。`Report` の列では 2 件目以降を
/// [`miette::Diagnostic::related`] へ載せられず、呼び出し側も error と warning で違う反復 API を
/// 使うことになる（#550）。
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
/// 相対 `root` は読み込みの前に `base_dir` を基準に解決するため、`Compilation.dependencies.config_path`
/// と診断が示す設定ファイルパスは解決後の値になる。
/// 保存（PDF ファイルへの書き出し）は行わない — `Compilation.pdf_path` が指す先へ書き出すのは
/// 呼び出し元の責務とする。
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
  let _compile_span = info_span!("compile").entered();
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

  let total_elapsed = build_start.elapsed();
  let statistics = BuildStatistics {
    page_count: publication.pages().len(),
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
/// 警告は段が失敗しても捨てない — 段が返した警告は、その段や後段が失敗してもその時点で確定しているため
/// （#550）。`warnings` へ積むのは各段の戻り値だけで、段の内側から直接積む経路は作らない。失敗した実行では
/// 呼び出し元（[`compile`]）が積み終えた `warnings` を [`CompileFailure::with_warnings`] で添える。
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
  for warning in config_warnings {
    warnings.push(warning);
  }
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
  for warning in typeset_warnings {
    warnings.push(warning);
  }
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
///
/// `compile` の公開シグネチャ `(source, root, base_dir)` は維持し、`base_dir` を compiler 側で暗黙に
/// 取得しない（`std::env::current_dir()` を呼ばない）判断も維持する。相対 `root` はここで `base_dir`
/// 基準の絶対パスになり、`DependencyManifest::config_path` と config 読込診断には解決後の値が現れる
/// （#530 で受け入れた唯一の意味的な差分。CLI は `base_dir` に `current_dir` を渡すので指す実体は同じ）。
/// これとは別に、`PathResolver` の解決契約（字句的正規化）により、診断・manifest・ソース名に出る
/// パスは一様に正規化済みの表示になる（中間の `.` が消える）— こちらは差分ではなく契約の帰結。
fn resolve_root(root: &ProjectPath, base_dir: &Path) -> (PathResolver, ProjectPath) {
  let resolver = PathResolver::new(base_dir);
  let root = resolver.resolve(root);
  return (resolver, root);
}

/// 入力読込 phase を実行する（production / test 共通）。
///
/// 読込順序とエラー集約は [`input::load`] が所有し、この関数が持つのは phase span と完了 event だけ。
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
  let _phase = info_span!("input").entered();
  let stage_start = Instant::now();
  let (inputs, config_warnings) = input::load(source, root, resolver);
  if inputs.is_ok() {
    info!(config_path = %root, elapsed = ?stage_start.elapsed(), "入力を読込");
  }
  return (inputs.map_err(CompileFailure::from), config_warnings);
}

/// 検証済み入力から意味解析済み文書までの 2 phase（frontend / semantics）を実行する
/// （production / test 共通）。
///
/// 各 phase の span・完了 event・診断への変換をここが所有し、`compile` と
/// `layout_project_for_test`（テスト専用）は同じ実装を通る。組版（フォント資源の構築・
/// 配置・`Publication` への変換）は `typeset::compose` の内側にあり、この関数は関与しない。
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
    let _phase = info_span!("frontend").entered();
    let stage_start = Instant::now();
    let document = parse_project(inputs, resolver)?;
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
  return Ok(semantic_document);
}

/// 入力読込から組版までを production と同じ実装で通し、組版中間表現を取り出すテストヘルパ。
///
/// `Publication` へ変換すると失われる情報（anchor・索引語のページ帰属・脚注 fragment・
/// `PlacedBlock` の幾何）を検査するテストだけが使う。phase の処理は再実装せず
/// [`load_inputs`] / [`analyze_document`] / [`typeset::layout_for_test`] を呼ぶだけなので、
/// `input::load` の横断検証も組版の段順序も迂回できない。
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
  let (resolver, root) = resolve_root(root, base_dir);
  let (inputs, _config_warnings) = load_inputs(source, &root, &resolver);
  let inputs = inputs?;
  let semantic_document = analyze_document(source, &inputs, &resolver)?;
  return typeset::layout_for_test(
    source,
    inputs.config(),
    inputs.style(),
    inputs.geometry(),
    inputs.font_data(),
    &semantic_document,
  )
  .map_err(CompileFailure::from);
}

/// 全ソースをパースし、1 つの文書木（HIR）へまとめる。
///
/// 画像パスは frontend が `resolver` で解決して HIR へ格納する。意味解析（ラベル・`\ref`・カウンタ・
/// 引用キー）と CSL 整形は `semantics::analyze` が、画像パスの収集は `typeset::compose` が担う。
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
/// 戻り値はソースごとの HIR。プロジェクト全体の文書木への組み立ては呼び出し元が行う。
/// エラーは宣言順に並べ、先頭（最初に失敗したソースの leaf 診断）を主診断にする。
fn parse_all_sources(sources: &SourceSet, resolver: &PathResolver) -> Result<Vec<HirSource>, CompileFailure> {
  let mut parsed: Vec<HirSource> = Vec::new();
  let mut parse_errors: Vec<BoxedDiagnostic> = Vec::new();

  for (source_id, entry) in sources.iter() {
    match frontend::parse_source(&entry.content, source_id, resolver) {
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
