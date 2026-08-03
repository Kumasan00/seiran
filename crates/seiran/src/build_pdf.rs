//! 設定ファイルの `sources` から PDF を生成するパイプライン

mod back_matter;
mod body;
mod compile;
mod error;
mod footnote_numbering;
mod front_matter;
mod image_manifest;
mod image_resources;
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
  fs,
  path::{Path, PathBuf},
  sync::Arc,
  time::Instant,
};

#[cfg(test)]
use citation::References;
use citation::read_references;
use compile::{LaidOutDocument, compile_project};
use error::{AttributedParseError, BuildPdfError};
use font::{FontData, FontDataExt, FontResources};
use image_manifest::ImageManifest;
use model::DocNode;
use project::{OutputPlan, ProjectSnapshot, SourceDb};
use semantics::SemanticsError;
use tracing::info;

/// ビルド成功時に表示するサマリ。
pub(super) struct BuildSummary {
  /// 出力した PDF のパス
  pub(super) output_path: PathBuf,
  /// 総ページ数
  pub(super) page_count: usize,
  /// ビルド全体の所要ミリ秒
  pub(super) total_elapsed_ms: u64,
}

/// 設定ファイルの `sources` から PDF を生成する。
///
/// 読み込み、パース、組版、描画、保存の各段を順に実行する。
pub(super) fn build_pdf(config_path: &Path) -> miette::Result<BuildSummary> {
  let build_start = Instant::now();
  info!(config_path = %config_path.display(), "PDF のビルドを開始します");

  let source = config::FilesystemProjectSource::new();
  let (snapshot, output) = load_project(&source, config_path)?;
  let (parsed, image_manifest) = parse_project(&snapshot)?;
  let resolved = semantics::resolve_semantics(&source, parsed, &snapshot.references, &snapshot.style)
    .map_err(|error| return wrap_semantics_error(error, &snapshot.source_db))?;
  let image_resources = image_resources::load_image_resources(&source, &image_manifest.paths)?;
  let font_resources = FontResources::load(&snapshot.config.font_configs, &snapshot.font_data)?;
  let font_system = font_resources.system()?;
  let laid_out = compile_project(&snapshot, &resolved, &image_resources, &font_system)?;
  let pdf_bytes = render_pdf(
    &snapshot.config,
    &snapshot.font_data,
    &font_resources,
    image_resources.into_image_bytes(),
    &laid_out,
  )?;

  fs::create_dir_all(&snapshot.config.output.output_dir).map_err(|source| {
    return BuildPdfError::CreateOutputDir {
      path: snapshot.config.output.output_dir.display().to_string(),
      source,
    };
  })?;
  let stage_start = Instant::now();
  fs::write(&output.pdf_path, pdf_bytes).map_err(|source| {
    return BuildPdfError::WritePdf {
      path: output.pdf_path.display().to_string(),
      source,
    };
  })?;
  info!(output_path = %output.pdf_path.display(), elapsed_ms = elapsed_ms(stage_start), "PDF の保存が完了しました");

  return Ok(BuildSummary {
    output_path: output.pdf_path,
    page_count: laid_out.pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
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
) -> miette::Result<(ProjectSnapshot, OutputPlan)> {
  let current_dir = std::env::current_dir().map_err(|source| return BuildPdfError::CurrentDir { source })?;
  let config = config::read_config(source, config_path, &current_dir)?;
  let style = config::read_style(source, config.style_path.as_deref(), &current_dir)?;
  config::validate_layout(&config, &style).map_err(|source| return BuildPdfError::Layout { source })?;
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

/// 構築済みフォント資源と画像バイト列から `Publication` を組み立て、PDF バイト列へ描画する。
///
/// フォント資源は呼び出し元が 1 回だけ構築したものをそのまま使う（ここでの再構築はしない）。
///
/// # Errors
///
/// `pdf_gen::ResourceBundle` の構築、または `pdf_gen::render` の描画に失敗した場合にエラーを返す。
fn render_pdf(
  config: &config::Config,
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  image_bytes: HashMap<model::AssetId, Vec<u8>>,
  laid_out: &LaidOutDocument,
) -> miette::Result<Vec<u8>> {
  let font_resource_configs = build_font_resource_configs(&config.font_configs);
  let resources = pdf_gen::ResourceBundle::new(
    &font_resource_configs,
    font_data,
    font_resources.font_refs(),
    font_resources.metrics().clone(),
    image_bytes,
  )?;
  let publication = publication::build_publication(config, resources, laid_out);

  let stage_start = Instant::now();
  let pdf_bytes = pdf_gen::render(&publication)?;
  info!(page_count = laid_out.pages.len(), elapsed_ms = elapsed_ms(stage_start), "PDF の描画が完了しました");

  return Ok(pdf_bytes);
}

/// `config::FontConfigs` から `pdf_gen::FontResourceConfigs`（config 非依存の複製）を組む。
fn build_font_resource_configs(font_configs: &config::FontConfigs) -> pdf_gen::FontResourceConfigs {
  return model::FontMap::from_all(model::FontType::ALL.iter().map(|font_type| {
    let font_config = font_configs.get(*font_type);
    return pdf_gen::FontResourceConfig {
      font_index: font_config.font_index,
      variation_axes: font_config.variation_axes.as_ref().map(|axes| {
        return axes
          .iter()
          .map(|axis| {
            return pdf_gen::VariationAxisConfig {
              name: axis.name,
              value: axis.value,
            };
          })
          .collect();
      }),
    };
  }));
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
  return compile_project(&snapshot, &resolved, &image_resources, &font_system);
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
fn parse_all_sources(
  source_db: &SourceDb,
  citation_keys: &HashSet<String>,
) -> Result<Vec<ParsedSource>, BuildPdfError> {
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
    return Err(BuildPdfError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(parsed);
}

/// resolve エラーが帰属するソースを `SourceDb` から引き当てる。
///
/// `SourceId` は `SourceDb::register` が発行した値をそのまま運んでいるため、
/// ここでの参照は確定 ID による引き当てであり、帰属元の推定ではない。
fn wrap_resolve_error(error: resolve::ResolveError, source_db: &SourceDb) -> BuildPdfError {
  return match error.origin() {
    model::Origin::Source(source_id) => {
      let entry = source_db.get(source_id);
      BuildPdfError::Resolve {
        src: miette::NamedSource::new(&entry.name, entry.content.clone()),
        source: error,
      }
    },
    model::Origin::Generated(_) => BuildPdfError::ResolveInternal { source: error },
  };
}

/// `semantics::resolve_semantics` のエラーを `BuildPdfError` へ変換する。
///
/// citation 由来はそのまま `Citation` へ、resolve 由来は `wrap_resolve_error` に委譲し、
/// 帰属ソースの有無で `Resolve` / `ResolveInternal` に振り分ける（従来の挙動を維持する）。
fn wrap_semantics_error(error: SemanticsError, source_db: &SourceDb) -> BuildPdfError {
  return match error {
    SemanticsError::Citation(source) => BuildPdfError::Citation { source },
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
