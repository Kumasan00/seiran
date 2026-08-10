//! 設定ファイルの `sources` から PDF を生成するパイプライン

mod dependency_manifest;
mod diagnostic_set;
mod error;
mod input;
mod publication;

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

pub use dependency_manifest::DependencyManifest;
pub use diagnostic_set::DiagnosticSet;
use error::{AttributedCitationError, AttributedParseError, CompileError};
use input::CompilationInputs;
pub use input::OutputPlan;
use tracing::info;

#[cfg(test)]
use crate::{project::config::ProjectConfig, semantics::References, style::Style, typeset::LaidOutDocument};
use crate::{
  project::{FontData, FontMap, SourceSet},
  publication::{Publication, PublicationFont, PublicationResources},
  semantics::AnalyzeError,
  typeset::FontResources,
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
  /// 致命的ではない診断（現状は常に空 — パイプラインに非致命的診断は存在しない）
  pub warnings: DiagnosticSet,
  /// コンパイル結果の統計情報
  pub statistics: BuildStatistics,
  /// 保存先など、書き込みを行う呼び出し側だけが使う出力情報
  pub output: OutputPlan,
}

/// `source` と `root`（設定ファイルパス）から PDF 直前の `Publication` までを 1 回で作る。
///
/// 言語処理・意味解決・組版を内部で順に実行する。呼び出し元は各段の中間型を知らない。
/// 保存（PDF ファイルへの書き出し）は行わない — `Compilation.output` が指す先へ書き出すのは
/// 呼び出し元の責務とする。
///
/// # Errors
///
/// 設定・ソース・文献・フォント・画像の読込、パース、意味解決、組版のいずれかに失敗した場合、
/// 診断の集合を返す。
pub fn compile<S: crate::project::ProjectSource>(
  source: &S,
  root: &crate::project::ProjectPath,
) -> Result<Compilation, DiagnosticSet> {
  return compile_inner(source, root).map_err(DiagnosticSet::from);
}

/// カレントディレクトリを解決してから [`compile_with_base_dir`] へ委譲する。
fn compile_inner<S: crate::project::ProjectSource>(
  source: &S,
  root: &crate::project::ProjectPath,
) -> miette::Result<Compilation> {
  let base_dir = std::env::current_dir().map_err(|source| return CompileError::CurrentDir { source })?;
  return compile_with_base_dir(source, root, &base_dir);
}

/// `base_dir` を注入できる `compile` の本体。
///
/// `MemoryProjectSource` + 固定 `base_dir` でのテストが `std::env::set_current_dir` 無しに
/// 書けるよう、テストと `compile` の双方から呼ばれる実処理をここに閉じる。
fn compile_with_base_dir<S: crate::project::ProjectSource>(
  source: &S,
  root: &crate::project::ProjectPath,
  base_dir: &Path,
) -> miette::Result<Compilation> {
  let build_start = Instant::now();
  info!(config_path = %root, "PDF のコンパイルを開始します");

  let inputs = input::load(source, root.as_path(), base_dir)?;
  let document = parse_project(&inputs)?;
  let semantic_document = crate::semantics::analyze(source, document, inputs.references(), inputs.style())
    .map_err(|error| return wrap_analyze_error(error, inputs.sources()))?;
  let font_resources = FontResources::load(&inputs.config().font_configs, inputs.font_data())?;
  let mut laid_out =
    crate::typeset::layout(source, inputs.config(), inputs.style(), &font_resources, &semantic_document)?;
  let image_bytes = std::mem::take(&mut laid_out.image_bytes);
  let resources = build_resources(inputs.font_data(), &font_resources, image_bytes);
  let publication = publication::build_publication(inputs.config(), resources, &laid_out);
  let dependencies = DependencyManifest::collect(root.as_path(), &inputs, &laid_out.image_paths);
  let statistics = BuildStatistics {
    page_count: laid_out.pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
  };

  return Ok(Compilation {
    publication,
    dependencies,
    warnings: DiagnosticSet::empty(),
    statistics,
    output: inputs.output().clone(),
  });
}

/// 全ソースをパースし、1 つの文書木（HIR）へまとめる。
///
/// 意味解析（ラベル・`\ref`・カウンタ・引用キー）と CSL 整形は `semantics::analyze` が、
/// 画像パスの収集は `typeset::layout` が担う。
///
/// # Errors
///
/// パース・評価エラーが集約して返る場合にエラーを返す。
fn parse_project(inputs: &CompilationInputs) -> miette::Result<crate::document::HirDocument> {
  let stage_start = Instant::now();
  let document = crate::document::HirDocument::assemble(parse_all_sources(inputs.sources())?);
  info!(
    source_count = document.groups().len(),
    node_count = document.groups().iter().map(|group| return group.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  return Ok(document);
}

/// 読み込み済みフォント資源と画像バイト列から `Publication` の描画資源を組み立てる。
///
/// フォント資源は呼び出し元が 1 回だけ構築したものをそのまま使う（ここでの再構築はしない）。
/// バイト列は `Arc` 共有なので複製しない。krilla フォントの構築は render（`seiran-pdf`）の責務。
fn build_resources(
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  image_bytes: HashMap<crate::project::ProjectPath, Vec<u8>>,
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
  let image_bytes: HashMap<String, Vec<u8>> =
    image_bytes.into_iter().map(|(path, bytes)| return (path.to_string(), bytes)).collect();
  return PublicationResources::new(fonts, image_bytes);
}

/// パースからページ確定までを実行するテストヘルパ（実ファイルシステム版）。
#[cfg(test)]
fn build_pages(
  config: &ProjectConfig,
  style: &Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
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
) -> miette::Result<LaidOutDocument> {
  let inputs =
    CompilationInputs::from_parts(source, config.clone(), style.clone(), Arc::clone(references), font_data.clone())?;
  let document = parse_project(&inputs)?;
  let semantic_document = crate::semantics::analyze(source, document, inputs.references(), inputs.style())
    .map_err(|error| return wrap_analyze_error(error, inputs.sources()))?;
  let font_resources = FontResources::load(&config.font_configs, font_data)?;
  return Ok(crate::typeset::layout(
    source,
    inputs.config(),
    inputs.style(),
    &font_resources,
    &semantic_document,
  )?);
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
///
/// ビルド処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 全ソースをパースし、パース・評価エラーを集約する。
///
/// 戻り値はソースごとの HIR。プロジェクト全体の文書木への組み立ては呼び出し元が行う。
// NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
#[allow(clippy::result_large_err)]
fn parse_all_sources(sources: &SourceSet) -> Result<Vec<crate::document::HirSource>, CompileError> {
  let mut parsed: Vec<crate::document::HirSource> = Vec::new();
  let mut parse_errors: Vec<AttributedParseError> = Vec::new();

  for (source_id, entry) in sources.iter() {
    match crate::frontend::parse_source(&entry.content, source_id) {
      Ok(hir) => parsed.push(hir),
      Err(error) => {
        parse_errors
          .push(AttributedParseError::new(miette::NamedSource::new(&entry.name, entry.content.clone()), error));
      },
    }
  }

  if !parse_errors.is_empty() {
    return Err(CompileError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(parsed);
}

/// 意味解析エラーが帰属するソースを `SourceSet` から引き当てる。
///
/// `SourceId` は `SourceSet::register` が発行した値をそのまま運んでいるため、
/// ここでの参照は確定 ID による引き当てであり、帰属元の推定ではない。`analyze` は実ソースしか
/// 走査しないので、帰属先が実ソース以外になることはない。
fn wrap_resolve_error(error: crate::semantics::SemanticError, sources: &SourceSet) -> CompileError {
  // 未定義引用キーは箇所ごとに `SourceId` を持ち、ソースごとの位置付き診断へ組み替える。
  if let crate::semantics::SemanticError::UnknownCitationKeys { sites } = error {
    return wrap_unknown_citation_keys(sites, sources);
  }
  let Some(source_id) = error.source_id() else {
    unreachable!("SourceId を持たないのは UnknownCitationKeys だけで、上で分岐済み: {error:?}")
  };
  let entry = sources.get(source_id);
  return CompileError::Resolve {
    src: miette::NamedSource::new(&entry.name, entry.content.clone()),
    source: error,
  };
}

/// 未定義引用キーのエラーを、ソースごとの位置付き診断へ変換する。
///
/// `UnknownCitationSite::source_id` は `SourceSet::register` が発行した ID をそのまま運んでいる
/// ため、ここでの参照は確定 ID による引き当てであり帰属元の推定ではない。
fn wrap_unknown_citation_keys(sites: Vec<crate::semantics::UnknownCitationSite>, sources: &SourceSet) -> CompileError {
  // ソースごとに 1 診断へまとめる（同じソース内の複数箇所はラベルを並べる）。
  // 出現順を保つため、初出順の Vec に積んでから組み立てる。
  let mut order: Vec<crate::source::SourceId> = Vec::new();
  let mut per_source: HashMap<crate::source::SourceId, Vec<miette::LabeledSpan>> = HashMap::new();
  for site in sites {
    let labels = per_source.entry(site.source_id).or_insert_with(|| {
      order.push(site.source_id);
      return Vec::new();
    });
    let span = miette::SourceSpan::from((site.span.start as usize, site.span.len() as usize));
    labels.push(miette::LabeledSpan::new_with_span(
      Some(format!("未定義の引用キー: {}", site.keys.join(", "))),
      span,
    ));
  }

  let errors = order
    .into_iter()
    .map(|source_id| {
      let entry = sources.get(source_id);
      let Some(labels) = per_source.remove(&source_id) else {
        unreachable!("order には per_source へ登録した SourceId しか入らない")
      };
      return AttributedCitationError::new(miette::NamedSource::new(&entry.name, entry.content.clone()), labels);
    })
    .collect();
  return CompileError::MultipleCitationErrors { errors };
}

/// `semantics::analyze` のエラーを `CompileError` へ変換する。
///
/// CSL 由来はそのまま `CitationStyle` / `CitationFormat` へ、意味解析由来は `wrap_resolve_error` に
/// 委譲する（未定義引用キーはそこからさらにソースごとの位置付き診断へ組み替える）。
fn wrap_analyze_error(error: AnalyzeError, sources: &SourceSet) -> CompileError {
  return match error {
    AnalyzeError::CitationStyle(source) => CompileError::CitationStyle { source },
    AnalyzeError::CitationFormat(source) => CompileError::CitationFormat { source },
    AnalyzeError::Analyze(source) => wrap_resolve_error(source, sources),
  };
}
