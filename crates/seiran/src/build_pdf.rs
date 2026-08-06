//! 設定ファイルの `sources` から PDF を生成するパイプライン

mod back_matter;
mod body;
mod dependency_manifest;
mod diagnostic_set;
mod error;
mod footnote_numbering;
mod front_matter;
mod image_manifest;
mod image_resources;
mod layout;
mod outline;
mod page_values;
mod phase_context;
mod project;
mod publication;
mod running;
mod semantics;

#[cfg(test)]
mod diagnostics;
#[cfg(test)]
mod dump;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod pdf_structure;
#[cfg(test)]
mod project_source_equivalence;

use std::{collections::HashMap, path::Path, sync::Arc, time::Instant};

pub use dependency_manifest::DependencyManifest;
pub use diagnostic_set::DiagnosticSet;
use error::{AttributedCitationError, AttributedParseError, CompileError};
use image_manifest::ImageManifest;
use layout::{DocumentLayouter, LaidOutDocument};
pub use project::OutputPlan;
use project::{ProjectSnapshot, SourceDb};
use semantics::SemanticsError;
use tracing::info;

#[cfg(test)]
use crate::citation::References;
use crate::{
  citation::read_references,
  font::{FontData, FontDataExt, FontResources},
  model::DocNode,
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
  pub publication: seiran_pdf::Publication,
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
pub fn compile<S: crate::config::ProjectSource>(
  source: &S,
  root: &crate::config::ProjectPath,
) -> Result<Compilation, DiagnosticSet> {
  return compile_inner(source, root).map_err(DiagnosticSet::from);
}

/// カレントディレクトリを解決してから [`compile_with_base_dir`] へ委譲する。
fn compile_inner<S: crate::config::ProjectSource>(
  source: &S,
  root: &crate::config::ProjectPath,
) -> miette::Result<Compilation> {
  let base_dir = std::env::current_dir().map_err(|source| return CompileError::CurrentDir { source })?;
  return compile_with_base_dir(source, root, &base_dir);
}

/// `base_dir` を注入できる `compile` の本体。
///
/// `MemoryProjectSource` + 固定 `base_dir` でのテストが `std::env::set_current_dir` 無しに
/// 書けるよう、テストと `compile` の双方から呼ばれる実処理をここに閉じる。
fn compile_with_base_dir<S: crate::config::ProjectSource>(
  source: &S,
  root: &crate::config::ProjectPath,
  base_dir: &Path,
) -> miette::Result<Compilation> {
  let build_start = Instant::now();
  info!(config_path = %root, "PDF のコンパイルを開始します");

  let (snapshot, output) = load_project(source, root.as_path(), base_dir)?;
  let (document, parsed, image_manifest) = parse_project(&snapshot)?;
  let resolved = semantics::resolve_semantics(source, &document, parsed, &snapshot.references, &snapshot.style)
    .map_err(|error| return wrap_semantics_error(error, &snapshot.source_db))?;
  let image_resources = image_resources::load_image_resources(source, &image_manifest.paths)?;
  let font_resources = FontResources::load(&snapshot.config.font_configs, &snapshot.font_data)?;
  let font_system = font_resources.system()?;
  let laid_out =
    DocumentLayouter::new(&snapshot.config, &snapshot.style, &font_system).layout(&resolved, &image_resources)?;
  let publication = build_publication(
    &snapshot.config,
    &snapshot.font_data,
    &font_resources,
    image_resources.into_image_bytes(),
    &laid_out,
  )?;
  let dependencies = DependencyManifest::collect(root.as_path(), &snapshot, &image_manifest);
  let statistics = BuildStatistics {
    page_count: laid_out.pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
  };

  return Ok(Compilation {
    publication,
    dependencies,
    warnings: DiagnosticSet::empty(),
    statistics,
    output,
  });
}

/// 設定・スタイル・文献・フォントを読み込み、プロジェクトを組み立てる。
///
/// `source` は呼び出し元が 1 回だけ構築したものを受け取り、ここでは構築しない。
///
/// # Errors
///
/// 設定、文献、フォント、ソースの読み込みまたは検証に失敗した場合にエラーを返す。
fn load_project(
  source: &dyn crate::config::ProjectSource,
  config_path: &Path,
  base_dir: &Path,
) -> miette::Result<(ProjectSnapshot, OutputPlan)> {
  let config = crate::config::read_config(source, config_path, base_dir)?;
  let style = crate::config::read_style(source, config.style_path.as_deref(), base_dir)?;
  crate::config::validate_layout(&config, &style).map_err(|source| return CompileError::Layout { source })?;
  let references = Arc::new(read_references(source, config.references_path.as_deref())?);

  let stage_start = Instant::now();
  let font_data = FontData::new(source, &config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  let output = OutputPlan {
    pdf_path: config.output.pdf_path(),
  };
  let snapshot = ProjectSnapshot::assemble(source, config, style, references, font_data)?;

  return Ok((snapshot, output));
}

/// 全ソースをパースし、画像パス一覧を作る。
///
/// `\cite` の存在検証・CSL 整形・ラベル/`\ref`/カウンタの解決は `semantics::resolve_semantics` が
/// 担う（この関数はパースと画像パス収集のみを行う）。呼び出し元は `HirDocument` を
/// `resolve_semantics` の引用キー存在検証（`citation::analyze_citations`）にそのまま渡す。
///
/// # Errors
///
/// パース・評価エラーが集約して返る場合にエラーを返す。
fn parse_project(
  snapshot: &ProjectSnapshot,
) -> miette::Result<(crate::model::HirDocument, Vec<ParsedSource>, ImageManifest)> {
  let stage_start = Instant::now();
  let document = crate::model::HirDocument::assemble(parse_all_sources(&snapshot.source_db)?);
  // 後段（citation / resolve / typeset）はまだ `DocNode` を入力に取るので、ここで変換する。
  // この adapter は #325 で削除する。
  let parsed: Vec<ParsedSource> = document
    .groups()
    .iter()
    .map(|group| {
      return ParsedSource {
        source_id: group.source_id,
        nodes: crate::frontend::hir_group_to_doc_nodes(group, document.locations()),
      };
    })
    .collect();
  info!(
    source_count = parsed.len(),
    node_count = parsed.iter().map(|p| return p.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  let groups: Vec<&[DocNode]> = parsed.iter().map(|p| return p.nodes.as_slice()).collect();
  let image_manifest = image_manifest::collect_image_paths(&groups);

  return Ok((document, parsed, image_manifest));
}

/// 構築済みフォント資源と画像バイト列から `Publication` を組み立てる（描画・保存はしない）。
///
/// フォント資源は呼び出し元が 1 回だけ構築したものをそのまま使う（ここでの再構築はしない）。
///
/// # Errors
///
/// `seiran_pdf::ResourceBundle` の構築に失敗した場合にエラーを返す。
fn build_publication(
  config: &crate::config::Config,
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  image_bytes: HashMap<crate::model::AssetId, Vec<u8>>,
  laid_out: &LaidOutDocument,
) -> miette::Result<seiran_pdf::Publication> {
  let fonts = build_pdf_fonts(font_data, font_resources);
  let font_metrics = build_pdf_font_metrics(font_resources);
  let image_bytes: HashMap<String, Vec<u8>> =
    image_bytes.into_iter().map(|(path, bytes)| return (path.to_string(), bytes)).collect();
  let resources = seiran_pdf::ResourceBundle::new(fonts, font_metrics, image_bytes)?;
  return Ok(publication::build_publication(config, resources, laid_out));
}

/// `crate::model::FontType` を `seiran_pdf::FontType` へ変換する（19 種別、宣言順で 1:1 対応）。
fn to_pdf_font_type(font_type: crate::model::FontType) -> seiran_pdf::FontType {
  return match font_type {
    crate::model::FontType::Serif => seiran_pdf::FontType::Serif,
    crate::model::FontType::SerifBold => seiran_pdf::FontType::SerifBold,
    crate::model::FontType::SerifItalic => seiran_pdf::FontType::SerifItalic,
    crate::model::FontType::SerifBoldItalic => seiran_pdf::FontType::SerifBoldItalic,
    crate::model::FontType::SansSerif => seiran_pdf::FontType::SansSerif,
    crate::model::FontType::SansSerifBold => seiran_pdf::FontType::SansSerifBold,
    crate::model::FontType::SansSerifItalic => seiran_pdf::FontType::SansSerifItalic,
    crate::model::FontType::SansSerifBoldItalic => seiran_pdf::FontType::SansSerifBoldItalic,
    crate::model::FontType::Monospace => seiran_pdf::FontType::Monospace,
    crate::model::FontType::MonospaceBold => seiran_pdf::FontType::MonospaceBold,
    crate::model::FontType::MonospaceItalic => seiran_pdf::FontType::MonospaceItalic,
    crate::model::FontType::MonospaceBoldItalic => seiran_pdf::FontType::MonospaceBoldItalic,
    crate::model::FontType::Math => seiran_pdf::FontType::Math,
    crate::model::FontType::JapaneseSerif => seiran_pdf::FontType::JapaneseSerif,
    crate::model::FontType::JapaneseSerifBold => seiran_pdf::FontType::JapaneseSerifBold,
    crate::model::FontType::JapaneseSansSerif => seiran_pdf::FontType::JapaneseSansSerif,
    crate::model::FontType::JapaneseSansSerifBold => seiran_pdf::FontType::JapaneseSansSerifBold,
    crate::model::FontType::JapaneseMonospace => seiran_pdf::FontType::JapaneseMonospace,
    crate::model::FontType::JapaneseMonospaceBold => seiran_pdf::FontType::JapaneseMonospaceBold,
  };
}

/// `crate::font::FontData` + `crate::font::FontResources` から `seiran_pdf::ResourceBundle::new` に渡す
/// フォント構築設定一式を組み立てる（`crate::model::FontType` → `seiran_pdf::FontType` への変換込み）。
fn build_pdf_fonts(
  font_data: &FontData,
  font_resources: &FontResources<'_>,
) -> HashMap<seiran_pdf::FontType, seiran_pdf::FontFaceInput> {
  let face_configs = font_resources.face_configs();
  return crate::model::FontType::ALL
    .iter()
    .map(|&font_type| {
      let face_config = face_configs.get(font_type);
      let input = seiran_pdf::FontFaceInput {
        bytes: font_data.get(font_type).clone(),
        font_index: face_config.font_index,
        variation_axes: face_config.variation_axes.as_ref().map(|axes| {
          return axes
            .iter()
            .map(|axis| {
              return seiran_pdf::VariationAxisInput {
                name: axis.name,
                value: axis.value,
              };
            })
            .collect();
        }),
      };
      return (to_pdf_font_type(font_type), input);
    })
    .collect();
}

/// `crate::font::FontResources` から `seiran_pdf::ResourceBundle::new` に渡すフォントメトリクス一式を組み立てる。
fn build_pdf_font_metrics(font_resources: &FontResources<'_>) -> HashMap<seiran_pdf::FontType, seiran_pdf::FontMetric> {
  let metrics = font_resources.metrics();
  return crate::model::FontType::ALL
    .iter()
    .map(|&font_type| {
      let metric = metrics.get(font_type);
      let converted = seiran_pdf::FontMetric {
        upem: metric.upem,
        ascender: metric.ascender,
        descender: metric.descender,
      };
      return (to_pdf_font_type(font_type), converted);
    })
    .collect();
}

/// パースからページ確定までを実行するテストヘルパ（実ファイルシステム版）。
#[cfg(test)]
fn build_pages(
  config: &crate::config::Config,
  style: &crate::config::Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  let source = crate::config::FilesystemProjectSource::new();
  return build_pages_with_source(&source, config, style, references, font_data);
}

/// パースからページ確定までを、指定した [`crate::config::ProjectSource`] 経由で実行するテストヘルパ。
///
/// `MemoryProjectSource` を渡すと実ファイルシステムに触れずに組版できる
/// （2 実装が同じ結果を返すことの検証用。issue #300）。
#[cfg(test)]
fn build_pages_with_source(
  source: &dyn crate::config::ProjectSource,
  config: &crate::config::Config,
  style: &crate::config::Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  let snapshot =
    ProjectSnapshot::assemble(source, config.clone(), style.clone(), Arc::clone(references), font_data.clone())?;
  let (document, parsed, image_manifest) = parse_project(&snapshot)?;
  let resolved = semantics::resolve_semantics(source, &document, parsed, &snapshot.references, &snapshot.style)
    .map_err(|error| return wrap_semantics_error(error, &snapshot.source_db))?;
  let image_resources = image_resources::load_image_resources(source, &image_manifest.paths)?;
  let font_resources = FontResources::load(&config.font_configs, font_data)?;
  let font_system = font_resources.system()?;
  return DocumentLayouter::new(&snapshot.config, &snapshot.style, &font_system).layout(&resolved, &image_resources);
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
///
/// ビルド処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 1 ソースのパース結果。本文の表示名・内容は `SourceDb` が持つため、ここでは持たない。
struct ParsedSource {
  /// パース元ソースの識別子
  source_id: crate::model::SourceId,
  /// パース・評価済みの Document IR ノード列
  nodes: Vec<DocNode>,
}

/// 全ソースをパースし、パース・評価エラーを集約する。
///
/// 戻り値はソースごとの HIR。プロジェクト全体の文書木への組み立ては呼び出し元が行う。
// NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
#[allow(clippy::result_large_err)]
fn parse_all_sources(source_db: &SourceDb) -> Result<Vec<crate::model::HirSource>, CompileError> {
  let mut parsed: Vec<crate::model::HirSource> = Vec::new();
  let mut parse_errors: Vec<AttributedParseError> = Vec::new();

  for (source_id, entry) in source_db.iter() {
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

/// resolve エラーが帰属するソースを `SourceDb` から引き当てる。
///
/// `SourceId` は `SourceDb::register` が発行した値をそのまま運んでいるため、
/// ここでの参照は確定 ID による引き当てであり、帰属元の推定ではない。
fn wrap_resolve_error(error: crate::resolve::ResolveError, source_db: &SourceDb) -> CompileError {
  return match error.origin() {
    crate::model::Origin::Source(source_id) => {
      let entry = source_db.get(source_id);
      CompileError::Resolve {
        src: miette::NamedSource::new(&entry.name, entry.content.clone()),
        source: error,
      }
    },
    crate::model::Origin::Generated(_) => CompileError::ResolveInternal { source: error },
  };
}

/// 未定義引用キーのエラーを、ソースごとの位置付き診断へ変換する。
///
/// `UnknownCitationSite::source_id` は `SourceDb::register` が発行した ID をそのまま運んでいる
/// ため、ここでの参照は確定 ID による引き当てであり帰属元の推定ではない。
fn wrap_citation_semantic_error(error: crate::citation::CitationSemanticError, source_db: &SourceDb) -> CompileError {
  let crate::citation::CitationSemanticError::UnknownCitationKeys { sites } = error;
  // ソースごとに 1 診断へまとめる（同じソース内の複数箇所はラベルを並べる）。
  // 出現順を保つため、初出順の Vec に積んでから組み立てる。
  let mut order: Vec<crate::model::SourceId> = Vec::new();
  let mut per_source: HashMap<crate::model::SourceId, Vec<miette::LabeledSpan>> = HashMap::new();
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
      let entry = source_db.get(source_id);
      let Some(labels) = per_source.remove(&source_id) else {
        unreachable!("order には per_source へ登録した SourceId しか入らない")
      };
      return AttributedCitationError::new(miette::NamedSource::new(&entry.name, entry.content.clone()), labels);
    })
    .collect();
  return CompileError::MultipleCitationErrors { errors };
}

/// `semantics::resolve_semantics` のエラーを `CompileError` へ変換する。
///
/// citation 由来はそのまま `CitationStyle` / `CitationFormat` へ、resolve 由来は
/// `wrap_resolve_error` に委譲し、帰属ソースの有無で `Resolve` / `ResolveInternal` に振り分ける。
/// 未定義引用キーは `wrap_citation_semantic_error` でソースごとの位置付き診断へ変換する
/// （従来の挙動を維持する）。
fn wrap_semantics_error(error: SemanticsError, source_db: &SourceDb) -> CompileError {
  return match error {
    SemanticsError::CitationStyle(source) => CompileError::CitationStyle { source },
    SemanticsError::CitationFormat(source) => CompileError::CitationFormat { source },
    SemanticsError::Resolve(source) => wrap_resolve_error(source, source_db),
    SemanticsError::CitationSemantic(source) => wrap_citation_semantic_error(source, source_db),
  };
}

#[cfg(test)]
mod tests {
  /// 書誌（`generated.bibliography`）に未解決 `\ref` を仕込み、`Origin::Generated` に帰属する resolve エラーを作る。
  pub(super) fn resolve_error_attributed_to_bibliography(style: &crate::config::Style) -> crate::resolve::ResolveError {
    use crate::model::{DocNode, InlineNode};
    let g0 = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "plain".to_string(),
    )])];
    let bibliography = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: crate::model::Span::DUMMY,
    }])];
    let citation_displays = crate::model::NodeMap::default();
    let semantic = crate::resolve::SemanticDocument {
      groups: vec![crate::resolve::SemanticGroup {
        nodes: &g0,
        source_id: crate::model::SourceId::new(0),
      }],
      generated: crate::resolve::SemanticGenerated {
        citation_displays: &citation_displays,
        bibliography: &bibliography,
      },
    };
    let error = crate::resolve::resolve_project(&semantic, style).expect_err("未定義ラベルはエラーになるはず");
    assert_eq!(
      error.origin(),
      crate::model::Origin::Generated(crate::model::GeneratedOrigin::Bibliography),
      "書誌が帰属源のはず"
    );
    return error;
  }
}
