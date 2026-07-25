//! 設定ファイルの `sources` から PDF を生成するパイプライン

mod back_matter;
mod body;
mod compile;
mod error;
mod footnote_numbering;
mod front_matter;
mod image_manifest;
mod outline;
mod page_values;
mod phase_context;
mod project;
mod publication;
mod running;

#[cfg(test)]
mod diagnostics;
#[cfg(test)]
mod dump;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod pdf_structure;

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
use error::BuildPdfError;
use font::{FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt};
use frontend::ParseSourceError;
use image_manifest::ImageManifest;
use model::DocNode;
use project::{OutputPlan, ProjectSnapshot};
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

  let (snapshot, output) = load_project(config_path)?;
  let (parsed_project, image_manifest) = parse_project(&snapshot)?;
  let image_set = pdf_gen::load_image_set(&image_manifest.paths)?;
  let font_refs = FontRefs::new(&snapshot.config.font_configs, &snapshot.font_data)?;
  let font_metrics = FontMetrics::new(&font_refs)?;
  let laid_out = compile_project(&snapshot, &parsed_project, &image_set, &font_refs, &font_metrics)?;
  let pdf_bytes = render_pdf(
    &snapshot.config,
    &snapshot.font_data,
    &font_refs,
    font_metrics,
    image_set.into_image_bytes(),
    &laid_out,
  )?;

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
/// # Errors
///
/// 設定、文献、フォント、ソースの読み込みまたは検証に失敗した場合にエラーを返す。
fn load_project(config_path: &Path) -> miette::Result<(ProjectSnapshot, OutputPlan)> {
  let config = config::read_config(config_path)?;
  let style = config::read_style(config.style_path.as_deref())?;
  config::validate_layout(&config, &style).map_err(|source| return BuildPdfError::Layout { source })?;
  let references = Arc::new(read_references(config.references_path.as_deref())?);

  let stage_start = Instant::now();
  let font_data = FontData::new(&config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  let output = OutputPlan {
    pdf_path: config.output.pdf_path(),
  };
  let snapshot = ProjectSnapshot::assemble(config, style, references, font_data)?;

  return Ok((snapshot, output));
}

/// ソースごとのパース結果と CSL で整形した書誌。
struct ParsedProject {
  /// ファイルごとのパース結果（ソース帰属を保持したまま、平坦化しない）
  parsed: Vec<ParsedSource>,
  /// `\cite` の CSL 整形で生成した書誌（合成グループとして groups の末尾に連結する）
  bibliography: Vec<DocNode>,
}

impl ParsedProject {
  /// 各ソースと末尾の書誌を `DocNode` のグループ列として返す。
  fn groups(&self) -> Vec<&[DocNode]> {
    return self
      .parsed
      .iter()
      .map(|p| return p.nodes.as_slice())
      .chain(std::iter::once(self.bibliography.as_slice()))
      .collect();
  }

  /// lowering に渡す起源付きのグループ列を組み立てる。
  ///
  /// 書誌には実ソースと区別できる `Origin::Generated` を割り当てる。
  fn lowering_groups(&self) -> Vec<typeset::SourceGroup<'_>> {
    let real_sources = self.parsed.iter().enumerate().map(|(i, p)| {
      return typeset::SourceGroup {
        nodes: p.nodes.as_slice(),
        origin: model::Origin::Source(model::SourceId::new(i)),
      };
    });
    let bibliography = std::iter::once(typeset::SourceGroup {
      nodes: self.bibliography.as_slice(),
      origin: model::Origin::Generated(model::GeneratedOrigin::Bibliography),
    });
    return real_sources.chain(bibliography).collect();
  }
}

/// 全ソースをパースし、書誌と画像パス一覧を作る。
///
/// `snapshot` の読込済みデータだけを使い、ファイル I/O は行わない。
///
/// # Errors
///
/// パース・評価エラーまたは CSL 整形に失敗した場合にエラーを返す。
fn parse_project(snapshot: &ProjectSnapshot) -> miette::Result<(ParsedProject, ImageManifest)> {
  let citation_keys: HashSet<String> = snapshot.references.keys().cloned().collect();

  let stage_start = Instant::now();
  let mut parsed = parse_all_sources(&snapshot.source_map, &citation_keys)?;
  info!(
    source_count = parsed.len(),
    node_count = parsed.iter().map(|p| return p.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  let stage_start = Instant::now();
  let bibliography =
    citation::process_citations(parsed.iter_mut().map(|p| return &mut p.nodes), &snapshot.references, &snapshot.style)
      .map_err(|source| return BuildPdfError::Citation { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "文献引用の CSL 整形が完了しました");

  let parsed_project = ParsedProject {
    parsed,
    bibliography,
  };
  let image_manifest = image_manifest::collect_image_paths(&parsed_project.groups());

  return Ok((parsed_project, image_manifest));
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
  font_refs: &FontRefs<'_>,
  font_metrics: FontMetrics,
  image_bytes: HashMap<model::AssetId, Vec<u8>>,
  laid_out: &LaidOutDocument,
) -> miette::Result<Vec<u8>> {
  let font_resource_configs = build_font_resource_configs(&config.font_configs);
  let resources =
    pdf_gen::ResourceBundle::new(&font_resource_configs, font_data, font_refs, font_metrics, image_bytes)?;
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

/// パースからページ確定までを実行するテストヘルパ。
#[cfg(test)]
fn build_pages(
  config: &config::Config,
  style: &config::Style,
  references: &Arc<References>,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  let snapshot = ProjectSnapshot::assemble(config.clone(), style.clone(), Arc::clone(references), font_data.clone())?;
  let (parsed_project, image_manifest) = parse_project(&snapshot)?;
  let image_set = pdf_gen::load_image_set(&image_manifest.paths)?;
  let font_refs = FontRefs::new(&config.font_configs, font_data)?;
  let font_metrics = FontMetrics::new(&font_refs)?;
  return compile_project(&snapshot, &parsed_project, &image_set, &font_refs, &font_metrics);
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
///
/// ビルド処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 1 ソースのパース結果と診断用の元テキスト。
struct ParsedSource {
  /// 表示用のソースパス文字列（`NamedSource` の名前になる）
  name: String,
  /// ソースファイルの元テキスト全体（`NamedSource` のスニペット元になる）
  content: String,
  /// パース・評価済みの Document IR ノード列
  nodes: Vec<DocNode>,
}

/// 全ソースをパースし、パース・評価エラーを集約する。
// NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
#[allow(clippy::result_large_err)]
fn parse_all_sources(
  source_map: &project::SourceMap,
  citation_keys: &HashSet<String>,
) -> Result<Vec<ParsedSource>, BuildPdfError> {
  let mut parsed: Vec<ParsedSource> = Vec::new();
  let mut parse_errors: Vec<ParseSourceError> = Vec::new();

  for entry in &source_map.sources {
    match frontend::parse_source(&entry.content, &entry.name, citation_keys) {
      Ok(nodes) => parsed.push(ParsedSource {
        name: entry.name.clone(),
        content: entry.content.clone(),
        nodes,
      }),
      Err(error) => parse_errors.push(error),
    }
  }

  if !parse_errors.is_empty() {
    return Err(BuildPdfError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(parsed);
}

/// lowering エラーに起源となるソースを紐付ける。
///
/// 合成されたノードが起源なら、実ソースを持たない内部エラーとして扱う。
fn wrap_lowering_error(error: typeset::LoweringError, parsed: &[ParsedSource]) -> BuildPdfError {
  return match error.origin() {
    model::Origin::Source(source_id) => {
      let source = parsed
        .get(source_id.index())
        .expect("Origin::Source は lowering_groups() が割り当てた実ソース配列の範囲内を指すはず");
      BuildPdfError::Lowering {
        src: miette::NamedSource::new(&source.name, source.content.clone()),
        source: error,
      }
    },
    model::Origin::Generated(_) => BuildPdfError::LoweringInternal { source: error },
  };
}

#[cfg(test)]
mod tests {
  use super::{BuildPdfError, ParsedSource, wrap_lowering_error};

  /// 指定した起源を持つ未定義参照の lowering エラーを作る。
  pub(super) fn lowering_error_with_origin(style: &config::Style, origin: model::Origin) -> typeset::LoweringError {
    use model::{DocNode, InlineNode};
    let ctx = typeset::LoweringContext::new(style);
    let g0 = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "plain".to_string(),
    )])];
    let g1 = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: model::Span::DUMMY,
    }])];
    let groups = [
      typeset::SourceGroup {
        nodes: &g0,
        origin: model::Origin::Source(model::SourceId::new(0)),
      },
      typeset::SourceGroup { nodes: &g1, origin },
    ];
    let error = typeset::lower_sources_with_headings(&ctx, &groups).expect_err("未定義ラベルはエラーになるはず");
    assert_eq!(error.origin(), origin, "指定した起源が帰属源のはず");
    return error;
  }

  #[test]
  fn lowering_error_attributes_named_source_for_real_source_origin() {
    // Arrange — Origin::Source(SourceId::new(1)) に帰属する LoweringError を生成する
    let style = config::Style::default();
    let parsed = vec![
      ParsedSource {
        name: "a.sei".to_string(),
        content: "A".to_string(),
        nodes: Vec::new(),
      },
      ParsedSource {
        name: "b.sei".to_string(),
        content: "B content".to_string(),
        nodes: Vec::new(),
      },
    ];
    let error = lowering_error_with_origin(&style, model::Origin::Source(model::SourceId::new(1)));

    // Act / Assert — index=1 の 2 番目のファイルに NamedSource が紐づく
    match wrap_lowering_error(error, &parsed) {
      BuildPdfError::Lowering { src, .. } => {
        assert_eq!(src.name(), "b.sei", "SourceId(1) は 2 番目のファイルに帰属するはず");
      },
      other => panic!("Lowering が期待されます: {other:?}"),
    }
  }

  #[test]
  fn lowering_error_falls_back_to_internal_for_generated_origin() {
    // Arrange — 合成グループに帰属する LoweringError を生成する
    let style = config::Style::default();
    let parsed = vec![ParsedSource {
      name: "only.sei".to_string(),
      content: "X".to_string(),
      nodes: Vec::new(),
    }];
    let error = lowering_error_with_origin(&style, model::Origin::Generated(model::GeneratedOrigin::Bibliography));

    // Act / Assert
    assert!(
      matches!(wrap_lowering_error(error, &parsed), BuildPdfError::LoweringInternal { .. }),
      "合成グループ（Origin::Generated）は LoweringInternal になるはず"
    );
  }
}
