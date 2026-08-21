//! 設定ファイルの `sources` から PDF を生成するパイプライン

mod compile_failure;
mod dependency_manifest;
mod error;
mod input;
mod publication;
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
use std::sync::Arc;
use std::{collections::HashMap, path::Path, time::Instant};

pub use compile_failure::CompileFailure;
pub use dependency_manifest::DependencyManifest;
use input::CompilationInputs;
pub use input::OutputPlan;
use source_diagnostic::SourceDiagnostic;
use tracing::info;
pub use warnings::Warnings;

#[cfg(test)]
use crate::{project::config::ProjectConfig, semantics::References, style::Style, typeset::LaidOutDocument};
use crate::{
  project::{FontData, FontMap, SourceSet},
  publication::{Publication, PublicationFont, PublicationImage, PublicationResources},
  semantics::AnalyzeError,
  typeset::{FontResources, FontWarning, ImageAsset, TypesetWarning},
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
  /// 保存先など、書き込みを行う呼び出し側だけが使う出力情報
  pub output: OutputPlan,
}

/// `source`、`root`（設定ファイルパス）、`base_dir`（相対パスの解決基準）から
/// PDF 直前の `Publication` までを 1 回で作る。
///
/// 言語処理・意味解決・組版を内部で順に実行する。呼び出し元は各段の中間型を知らない。
/// `base_dir` は呼び出し元が実行環境に応じて明示し、本関数はカレントディレクトリを取得しない。
/// 保存（PDF ファイルへの書き出し）は行わない — `Compilation.output` が指す先へ書き出すのは
/// 呼び出し元の責務とする。
///
/// # Errors
///
/// 設定・ソース・文献・フォント・画像の読込、パース、意味解決、組版のいずれかに失敗した場合、
/// 1 件以上の error diagnostic を持つ [`CompileFailure`] を返す。
pub fn compile<S: crate::project::ProjectSource>(
  source: &S,
  root: &crate::project::ProjectPath,
  base_dir: &Path,
) -> Result<Compilation, CompileFailure> {
  let build_start = Instant::now();
  info!(phase = "compile", config_path = %root, "PDF のコンパイルを開始します");

  let stage_start = Instant::now();
  let inputs = input::load(source, root.as_path(), base_dir)?;
  info!(phase = "input", elapsed_ms = elapsed_ms(stage_start), "入力の読み込みが完了しました");

  let stage_start = Instant::now();
  let document = parse_project(&inputs)?;
  info!(
    phase = "frontend",
    source_count = document.groups().len(),
    node_count = document.groups().iter().map(|group| return group.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "構文解析が完了しました"
  );

  let stage_start = Instant::now();
  let semantic_document = crate::semantics::analyze(source, document, inputs.references(), inputs.style())
    .map_err(|error| return attribute_analyze_error(error, inputs.sources()))?;
  info!(
    phase = "semantics",
    heading_count = semantic_document.headings().len(),
    elapsed_ms = elapsed_ms(stage_start),
    "意味解析が完了しました"
  );

  let stage_start = Instant::now();
  let (font_resources, font_warnings) =
    FontResources::load(&inputs.config().font_configs, inputs.font_data()).map_err(CompileFailure::from)?;
  info!(
    phase = "font",
    warning_count = font_warnings.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "フォント資源の構築が完了しました"
  );

  let stage_start = Instant::now();
  let (mut laid_out, typeset_warnings) =
    crate::typeset::layout(source, inputs.config(), inputs.style(), &font_resources, &semantic_document)
      .map_err(CompileFailure::from)?;
  info!(
    phase = "typeset",
    page_count = laid_out.pages.len(),
    warning_count = typeset_warnings.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "組版が完了しました"
  );
  let images = std::mem::take(&mut laid_out.images);
  let resources = build_resources(inputs.font_data(), &font_resources, images);
  let publication = publication::build_publication(inputs.config(), resources, &laid_out);
  let dependencies = DependencyManifest::collect(root.as_path(), &inputs, &laid_out.image_paths);
  let warnings = collect_warnings(&inputs, font_warnings, typeset_warnings);
  let total_elapsed_ms = elapsed_ms(build_start);
  let statistics = BuildStatistics {
    page_count: laid_out.pages.len(),
    total_elapsed_ms,
  };
  info!(
    phase = "compile",
    page_count = statistics.page_count,
    warning_count = warnings.reports().count(),
    elapsed_ms = total_elapsed_ms,
    "PDF のコンパイルが完了しました"
  );

  return Ok(Compilation {
    publication,
    dependencies,
    warnings,
    statistics,
    output: inputs.output().clone(),
  });
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
  let mut warnings = Warnings::empty();
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
fn parse_project(inputs: &CompilationInputs) -> Result<crate::document::HirDocument, CompileFailure> {
  let document = crate::document::HirDocument::assemble(parse_all_sources(inputs.sources())?);
  return Ok(document);
}

/// 読み込み済みフォント資源と画像資源から `Publication` の描画資源を組み立てる。
///
/// フォント資源は呼び出し元が 1 回だけ構築したものをそのまま使う（ここでの再構築はしない）。
/// バイト列は `Arc` 共有なので複製しない。krilla フォントの構築は render（`seiran-pdf`）の責務。
///
/// 画像はパス昇順に並べてから渡す — `ImageRef` は配列添字なので、`HashMap` の反復順のままだと
/// 同じ入力から作った `Publication` が実行ごとに違う値になってしまう。
fn build_resources(
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  images: HashMap<crate::project::ProjectPath, ImageAsset>,
) -> PublicationResources {
  let face_configs = font_resources.face_configs();
  let metrics = font_resources.metrics();
  let fonts = FontMap::from_all(crate::project::FontType::ALL.iter().map(|&font_type| {
    return PublicationFont {
      bytes: font_data.shared_bytes(font_type),
      face: face_configs.get(font_type).clone(),
      metric: *metrics.get(font_type),
    };
  }));
  let mut sorted: Vec<(crate::project::ProjectPath, ImageAsset)> = images.into_iter().collect();
  sorted.sort_by(|(left, _), (right, _)| return left.cmp(right));
  let images = sorted
    .into_iter()
    .map(|(path, asset)| {
      return PublicationImage {
        path: path.to_string(),
        format: asset.format,
        bytes: asset.bytes,
      };
    })
    .collect();
  return PublicationResources::new(fonts, images);
}

/// パースからページ確定までを実行するテストヘルパ（実ファイルシステム版）。
#[cfg(test)]
fn build_pages(
  config: &ProjectConfig,
  style: &Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> Result<LaidOutDocument, CompileFailure> {
  let source = crate::project::FilesystemProjectSource::new();
  return build_pages_with_source(&source, config, style, references, font_data);
}

/// パースからページ確定までを、指定した [`crate::project::ProjectSource`] 経由で実行するテストヘルパ。
///
/// `MemoryProjectSource` を渡すと実ファイルシステムに触れずに組版できる
/// （2 実装が同じ結果を返すことの検証用。issue #300）。
#[cfg(test)]
fn build_pages_with_source(
  source: &dyn crate::project::ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> Result<LaidOutDocument, CompileFailure> {
  let inputs =
    CompilationInputs::from_parts(source, config.clone(), style.clone(), Arc::clone(references), font_data.clone())?;
  let document = parse_project(&inputs)?;
  let semantic_document = crate::semantics::analyze(source, document, inputs.references(), inputs.style())
    .map_err(|error| return attribute_analyze_error(error, inputs.sources()))?;
  let (font_resources, _) = FontResources::load(&config.font_configs, font_data).map_err(CompileFailure::from)?;
  return crate::typeset::layout(source, inputs.config(), inputs.style(), &font_resources, &semantic_document)
    .map(|(laid_out, _)| return laid_out)
    .map_err(CompileFailure::from);
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
#[allow(clippy::cast_possible_truncation, reason = "経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはない")]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 全ソースをパースし、パース・評価エラーを集約する。
///
/// 戻り値はソースごとの HIR。プロジェクト全体の文書木への組み立ては呼び出し元が行う。
/// エラーは宣言順に並べ、先頭（最初に失敗したソースの leaf 診断）を主診断にする。
fn parse_all_sources(sources: &SourceSet) -> Result<Vec<crate::document::HirSource>, CompileFailure> {
  let mut parsed: Vec<crate::document::HirSource> = Vec::new();
  let mut parse_errors: Vec<Box<dyn miette::Diagnostic + Send + Sync + 'static>> = Vec::new();

  for (source_id, entry) in sources.iter() {
    match crate::frontend::parse_source(&entry.content, source_id) {
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
