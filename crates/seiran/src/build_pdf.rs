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

use std::{
  collections::{HashMap, HashSet},
  path::Path,
  sync::Arc,
  time::Instant,
};

pub use dependency_manifest::DependencyManifest;
pub use diagnostic_set::DiagnosticSet;
use error::{AttributedParseError, CompileError};
use font::{FontData, FontDataExt, FontResources};
use image_manifest::ImageManifest;
use layout::{DocumentLayouter, LaidOutDocument};
use model::DocNode;
pub use project::OutputPlan;
use project::{ProjectSnapshot, SourceDb};
use semantics::SemanticsError;
use tracing::info;

#[cfg(test)]
use crate::citation::References;
use crate::citation::read_references;

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
  pub publication: pdf_gen::Publication,
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
pub fn compile<S: config::ProjectSource>(source: &S, root: &config::ProjectPath) -> Result<Compilation, DiagnosticSet> {
  return compile_inner(source, root).map_err(DiagnosticSet::from);
}

/// カレントディレクトリを解決してから [`compile_with_base_dir`] へ委譲する。
fn compile_inner<S: config::ProjectSource>(source: &S, root: &config::ProjectPath) -> miette::Result<Compilation> {
  let base_dir = std::env::current_dir().map_err(|source| return CompileError::CurrentDir { source })?;
  return compile_with_base_dir(source, root, &base_dir);
}

/// `base_dir` を注入できる `compile` の本体。
///
/// `MemoryProjectSource` + 固定 `base_dir` でのテストが `std::env::set_current_dir` 無しに
/// 書けるよう、テストと `compile` の双方から呼ばれる実処理をここに閉じる。
fn compile_with_base_dir<S: config::ProjectSource>(
  source: &S,
  root: &config::ProjectPath,
  base_dir: &Path,
) -> miette::Result<Compilation> {
  let build_start = Instant::now();
  info!(config_path = %root, "PDF のコンパイルを開始します");

  let (snapshot, output) = load_project(source, root.as_path(), base_dir)?;
  let (parsed, image_manifest) = parse_project(&snapshot)?;
  let resolved = semantics::resolve_semantics(source, parsed, &snapshot.references, &snapshot.style)
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
  source: &dyn config::ProjectSource,
  config_path: &Path,
  base_dir: &Path,
) -> miette::Result<(ProjectSnapshot, OutputPlan)> {
  let config = config::read_config(source, config_path, base_dir)?;
  let style = config::read_style(source, config.style_path.as_deref(), base_dir)?;
  config::validate_layout(&config, &style).map_err(|source| return CompileError::Layout { source })?;
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
/// `\cite` の CSL 整形・ラベル/`\ref`/カウンタの解決は `semantics::resolve_semantics` が担う
/// （この関数はパースと画像パス収集のみを行う）。
///
/// # Errors
///
/// パース・評価エラーが集約して返る場合にエラーを返す。
fn parse_project(snapshot: &ProjectSnapshot) -> miette::Result<(Vec<ParsedSource>, ImageManifest)> {
  let citation_keys: HashSet<String> = snapshot.references.keys().cloned().collect();

  let stage_start = Instant::now();
  let parsed = parse_all_sources(&snapshot.source_db, &citation_keys)?;
  info!(
    source_count = parsed.len(),
    node_count = parsed.iter().map(|p| return p.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  let groups: Vec<&[DocNode]> = parsed.iter().map(|p| return p.nodes.as_slice()).collect();
  let image_manifest = image_manifest::collect_image_paths(&groups);

  return Ok((parsed, image_manifest));
}

/// 構築済みフォント資源と画像バイト列から `Publication` を組み立てる（描画・保存はしない）。
///
/// フォント資源は呼び出し元が 1 回だけ構築したものをそのまま使う（ここでの再構築はしない）。
///
/// # Errors
///
/// `pdf_gen::ResourceBundle` の構築に失敗した場合にエラーを返す。
fn build_publication(
  config: &config::Config,
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  image_bytes: HashMap<model::AssetId, Vec<u8>>,
  laid_out: &LaidOutDocument,
) -> miette::Result<pdf_gen::Publication> {
  let fonts = build_pdf_fonts(font_data, font_resources);
  let font_metrics = build_pdf_font_metrics(font_resources);
  let image_bytes: HashMap<String, Vec<u8>> =
    image_bytes.into_iter().map(|(path, bytes)| return (path.to_string(), bytes)).collect();
  let resources = pdf_gen::ResourceBundle::new(fonts, font_metrics, image_bytes)?;
  return Ok(publication::build_publication(config, resources, laid_out));
}

/// `model::FontType` を `pdf_gen::FontType` へ変換する（19 種別、宣言順で 1:1 対応）。
fn to_pdf_font_type(font_type: model::FontType) -> pdf_gen::FontType {
  return match font_type {
    model::FontType::Serif => pdf_gen::FontType::Serif,
    model::FontType::SerifBold => pdf_gen::FontType::SerifBold,
    model::FontType::SerifItalic => pdf_gen::FontType::SerifItalic,
    model::FontType::SerifBoldItalic => pdf_gen::FontType::SerifBoldItalic,
    model::FontType::SansSerif => pdf_gen::FontType::SansSerif,
    model::FontType::SansSerifBold => pdf_gen::FontType::SansSerifBold,
    model::FontType::SansSerifItalic => pdf_gen::FontType::SansSerifItalic,
    model::FontType::SansSerifBoldItalic => pdf_gen::FontType::SansSerifBoldItalic,
    model::FontType::Monospace => pdf_gen::FontType::Monospace,
    model::FontType::MonospaceBold => pdf_gen::FontType::MonospaceBold,
    model::FontType::MonospaceItalic => pdf_gen::FontType::MonospaceItalic,
    model::FontType::MonospaceBoldItalic => pdf_gen::FontType::MonospaceBoldItalic,
    model::FontType::Math => pdf_gen::FontType::Math,
    model::FontType::JapaneseSerif => pdf_gen::FontType::JapaneseSerif,
    model::FontType::JapaneseSerifBold => pdf_gen::FontType::JapaneseSerifBold,
    model::FontType::JapaneseSansSerif => pdf_gen::FontType::JapaneseSansSerif,
    model::FontType::JapaneseSansSerifBold => pdf_gen::FontType::JapaneseSansSerifBold,
    model::FontType::JapaneseMonospace => pdf_gen::FontType::JapaneseMonospace,
    model::FontType::JapaneseMonospaceBold => pdf_gen::FontType::JapaneseMonospaceBold,
  };
}

/// `font::FontData` + `font::FontResources` から `pdf_gen::ResourceBundle::new` に渡す
/// フォント構築設定一式を組み立てる（`model::FontType` → `pdf_gen::FontType` への変換込み）。
fn build_pdf_fonts(
  font_data: &FontData,
  font_resources: &FontResources<'_>,
) -> HashMap<pdf_gen::FontType, pdf_gen::FontFaceInput> {
  let face_configs = font_resources.face_configs();
  return model::FontType::ALL
    .iter()
    .map(|&font_type| {
      let face_config = face_configs.get(font_type);
      let input = pdf_gen::FontFaceInput {
        bytes: font_data.get(font_type).clone(),
        font_index: face_config.font_index,
        variation_axes: face_config.variation_axes.as_ref().map(|axes| {
          return axes
            .iter()
            .map(|axis| {
              return pdf_gen::VariationAxisInput {
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

/// `font::FontResources` から `pdf_gen::ResourceBundle::new` に渡すフォントメトリクス一式を組み立てる。
fn build_pdf_font_metrics(font_resources: &FontResources<'_>) -> HashMap<pdf_gen::FontType, pdf_gen::FontMetric> {
  let metrics = font_resources.metrics();
  return model::FontType::ALL
    .iter()
    .map(|&font_type| {
      let metric = metrics.get(font_type);
      let converted = pdf_gen::FontMetric {
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
  config: &config::Config,
  style: &config::Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  let source = config::FilesystemProjectSource::new();
  return build_pages_with_source(&source, config, style, references, font_data);
}

/// パースからページ確定までを、指定した [`config::ProjectSource`] 経由で実行するテストヘルパ。
///
/// `MemoryProjectSource` を渡すと実ファイルシステムに触れずに組版できる
/// （2 実装が同じ結果を返すことの検証用。issue #300）。
#[cfg(test)]
fn build_pages_with_source(
  source: &dyn config::ProjectSource,
  config: &config::Config,
  style: &config::Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  let snapshot =
    ProjectSnapshot::assemble(source, config.clone(), style.clone(), Arc::clone(references), font_data.clone())?;
  let (parsed, image_manifest) = parse_project(&snapshot)?;
  let resolved = semantics::resolve_semantics(source, parsed, &snapshot.references, &snapshot.style)
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
  source_id: model::SourceId,
  /// パース・評価済みの Document IR ノード列
  nodes: Vec<DocNode>,
}

/// 全ソースをパースし、パース・評価エラーを集約する。
// NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
#[allow(clippy::result_large_err)]
fn parse_all_sources(source_db: &SourceDb, citation_keys: &HashSet<String>) -> Result<Vec<ParsedSource>, CompileError> {
  let mut parsed: Vec<ParsedSource> = Vec::new();
  let mut parse_errors: Vec<AttributedParseError> = Vec::new();

  for (source_id, entry) in source_db.iter() {
    match frontend::parse_source(&entry.content, source_id, citation_keys) {
      Ok(nodes) => parsed.push(ParsedSource { source_id, nodes }),
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
fn wrap_resolve_error(error: resolve::ResolveError, source_db: &SourceDb) -> CompileError {
  return match error.origin() {
    model::Origin::Source(source_id) => {
      let entry = source_db.get(source_id);
      CompileError::Resolve {
        src: miette::NamedSource::new(&entry.name, entry.content.clone()),
        source: error,
      }
    },
    model::Origin::Generated(_) => CompileError::ResolveInternal { source: error },
  };
}

/// `semantics::resolve_semantics` のエラーを `CompileError` へ変換する。
///
/// citation 由来はそのまま `Citation` へ、resolve 由来は `wrap_resolve_error` に委譲し、
/// 帰属ソースの有無で `Resolve` / `ResolveInternal` に振り分ける（従来の挙動を維持する）。
fn wrap_semantics_error(error: SemanticsError, source_db: &SourceDb) -> CompileError {
  return match error {
    SemanticsError::Citation(source) => CompileError::Citation { source },
    SemanticsError::Resolve(source) => wrap_resolve_error(source, source_db),
  };
}

#[cfg(test)]
mod tests {
  /// 書誌（`bibliography` フィールド）に未解決 `\ref` を仕込み、`Origin::Generated` に帰属する resolve エラーを作る。
  pub(super) fn resolve_error_attributed_to_bibliography(style: &config::Style) -> resolve::ResolveError {
    use model::{DocNode, InlineNode};
    let g0 = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "plain".to_string(),
    )])];
    let bibliography = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: model::Span::DUMMY,
    }])];
    let semantic = resolve::SemanticDocument {
      groups: vec![resolve::SemanticGroup {
        nodes: &g0,
        source_id: model::SourceId::new(0),
      }],
      bibliography: &bibliography,
    };
    let error = resolve::resolve_project(&semantic, style).expect_err("未定義ラベルはエラーになるはず");
    assert_eq!(error.origin(), model::Origin::Generated(model::GeneratedOrigin::Bibliography), "書誌が帰属源のはず");
    return error;
  }
}
