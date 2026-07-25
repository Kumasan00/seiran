//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

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
  collections::HashSet,
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

/// ビルド成功時のサマリ（ユーザーチャンネルのレポータが表示する最小情報）
///
/// `tracing` の段ログとは別物で、コンパイラ型 CLI の「成果報告 1 行」（出力先・ページ数・所要
/// 時間）だけを運ぶ。表示は呼び出し側（`main::report_build`）が担う。
pub(super) struct BuildSummary {
  /// 出力した PDF のパス
  pub(super) output_path: PathBuf,
  /// 総ページ数
  pub(super) page_count: usize,
  /// ビルド全体の所要ミリ秒
  pub(super) total_elapsed_ms: u64,
}

/// 設定ファイルの `sources` から PDF を生成
///
/// `load_project`（設定・スタイル・文献・フォント・ソーステキストの読込 → `ProjectSnapshot` /
/// `OutputPlan`）→ `parse_project`（ソースのパース・`\cite` の CSL 整形・画像パス収集）→
/// （driver が `ImageManifest` の画像を読み `ImageSet` を作る）→ `compile_project`（lowering 〜
/// 走り文配置までの組版）→ `encode_pdf`（PDF バイト列への描画）→ 保存・報告、を順に呼ぶだけの
/// 薄い driver。各段の実処理・エラー戦略は各関数のドキュメントを参照。
///
/// # Arguments
///
/// * `config_path` - 設定ファイルのパス
///
/// # Returns
///
/// 成功時は [`BuildSummary`]（出力先・ページ数・所要時間）。ユーザー向けの成果報告は呼び出し側の
/// `report_build` が担うため、ここでは最終サマリを `tracing` には出さない（二重表示を避ける）。
pub(super) fn build_pdf(config_path: &Path) -> miette::Result<BuildSummary> {
  let build_start = Instant::now();
  info!(config_path = %config_path.display(), "PDF のビルドを開始します");

  let (snapshot, output) = load_project(config_path)?;
  let (parsed_project, image_manifest) = parse_project(&snapshot)?;
  let image_set = pdf_gen::load_image_set(&image_manifest.paths)?;
  let laid_out = compile_project(&snapshot, &parsed_project, &image_set)?;
  let pdf_bytes = encode_pdf(&snapshot.config, &snapshot.font_data, &laid_out)?;

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

/// 設定ファイル・スタイル・文献・フォントを読み込み、`ProjectSnapshot` / `OutputPlan` を組み立てる
/// （`build_pdf` の 1 段目）。
///
/// I/O とバリデーションだけを担い、ソースのパースや組版には踏み込まない。
///
/// # Errors
///
/// 設定・スタイルの読込／レイアウト検証、文献の読込、フォントの読込、ソースファイルの読込の
/// いずれかで失敗した場合にエラーを返す。
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

/// [`parse_project`] の出力＝ソースファイルごとのパース結果と、`\cite` を CSL 整形した書誌。
///
/// [`compile_project`] が groups（`Vec<&[DocNode]>`）を組み立てる元データであり、`parsed` は
/// lowering エラーのソース帰属（[`wrap_lowering_error`]）にも使う。
struct ParsedProject {
  /// ファイルごとのパース結果（ソース帰属を保持したまま、平坦化しない）
  parsed: Vec<ParsedSource>,
  /// `\cite` の CSL 整形で生成した書誌（合成グループとして groups の末尾に連結する）
  bibliography: Vec<DocNode>,
}

impl ParsedProject {
  /// 各ソースファイルを 1 グループとし、書誌を末尾の合成グループとして連結する。
  ///
  /// 画像パス収集（`image_manifest::collect_image_paths`）は起源を必要としないため、
  /// `DocNode` 列だけを返す。lowering に渡す起源付きの並びは [`ParsedProject::lowering_groups`] を使う。
  fn groups(&self) -> Vec<&[DocNode]> {
    return self
      .parsed
      .iter()
      .map(|p| return p.nodes.as_slice())
      .chain(std::iter::once(self.bibliography.as_slice()))
      .collect();
  }

  /// [`typeset::lower_sources_with_headings`] に渡す、起源付きのグループ列を組み立てる。
  ///
  /// 各ソースファイルには `Origin::Source(SourceId::new(i))` を、末尾に連結する書誌の合成グループには
  /// `Origin::Generated(GeneratedOrigin::Bibliography)` を明示的に割り当てる。「配列範囲外の
  /// `SourceId`」という暗黙の sentinel で合成グループを表さない（#259）。
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

/// 全ソースファイルをパースし、`\cite` を CSL 整形して書誌を作り、画像パス一覧を収集する
/// （`build_pdf` の 2 段目）。
///
/// `snapshot` の読込済みデータだけを見る純粋関数で、ファイル I/O を行わない
/// （ソースの読込は `ProjectSnapshot::assemble` が既に済ませている）。
///
/// # Errors
///
/// - パース・評価エラーは全ソースで集約して `MultipleSourceErrors` で報告
/// - `\cite` の CSL 整形に失敗した場合もエラーを返す
fn parse_project(snapshot: &ProjectSnapshot) -> miette::Result<(ParsedProject, ImageManifest)> {
  // `\cite` のキー存在検証に使う有効な参照 ID 集合（CSL 整形そのものは後続の citation ステージで実施）
  let citation_keys: HashSet<String> = snapshot.references.keys().cloned().collect();

  let stage_start = Instant::now();
  let mut parsed = parse_all_sources(&snapshot.source_map, &citation_keys)?;
  info!(
    source_count = parsed.len(),
    node_count = parsed.iter().map(|p| return p.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  // `\cite` を CSL 整形し、引用された文献の書誌を最後の合成グループとして受け取る（parser の後・lowering の前）。
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

/// 確定レイアウトを PDF バイト列へ描画する（`build_pdf` の 4 段目）。
///
/// # Errors
///
/// フォント参照の再構築、または `pdf_gen::create_pdf` の描画に失敗した場合にエラーを返す。
fn encode_pdf(config: &config::Config, font_data: &FontData, laid_out: &LaidOutDocument) -> miette::Result<Vec<u8>> {
  let font_refs = FontRefs::new(&config.font_configs, font_data)?;
  let metrics = FontMetrics::new(&font_refs)?;
  let publication = pdf_gen::PublicationBuilder::new(config).build(&laid_out.pages, &laid_out.outline_entries);

  let stage_start = Instant::now();
  let pdf_bytes = pdf_gen::create_pdf(&publication, font_data, &font_refs, &metrics, &config.font_configs)?;
  info!(page_count = laid_out.pages.len(), elapsed_ms = elapsed_ms(stage_start), "PDF の描画が完了しました");

  return Ok(pdf_bytes);
}

/// テスト専用: `parse_project` → 画像読込 → `compile_project` を通しで実行するヘルパ。
///
/// `build_pdf` 本体は `load_project` の結果からまず `parse_project` を呼び、`ImageManifest` の
/// 画像を読んでから `compile_project` へ渡す driver だが、golden / diagnostic / PDF 構造テストは
/// 分割前の 1 呼び出しインターフェース（旧 `build_pages`）を前提に書かれているため、ここで束ねて
/// 提供する。`config` / `style` / `font_data` は呼出元（`pdf_structure.rs` 等）が呼出後も使うため
/// 借用のまま受け取り、内部でだけ複製して `ProjectSnapshot` へ移す。
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
  return compile_project(&snapshot, &parsed_project, &image_set);
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
///
/// ビルド処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 1 ソースファイルのパース結果と、そのファイル名・内容（診断用）。
///
/// `parse_all_sources` が平坦化せずファイルごとに 1 件生成する。`nodes` の並び順（`Vec<ParsedSource>`
/// のインデックス）が [`typeset::SourceId`] に一致し、lowering エラー発生時に `name` / `content` を
/// `NamedSource` へ逆引きしてファイル名・スニペット付きの診断を表示するのに使う。
struct ParsedSource {
  /// 表示用のソースパス文字列（`NamedSource` の名前になる）
  name: String,
  /// ソースファイルの元テキスト全体（`NamedSource` のスニペット元になる）
  content: String,
  /// パース・評価済みの Document IR ノード列
  nodes: Vec<DocNode>,
}

/// `source_map` の全エントリをパースし、[`ParsedSource`] を生成して返す。
///
/// I/O は行わない（`SourceMap::read` が既に読込済み）。パース・評価エラーは全 source で
/// 集約して [`BuildPdfError::MultipleSourceErrors`] にまとめて返す。
// BuildPdfError は診断用の NamedSource を同梱するため大きい。ソース位置付き診断を優先する方針で、
// frontend::parse_source と同じく result_large_err を許可する（Err は稀な失敗時のみ構築される）。
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

/// `LoweringError` を、`origin()` で特定できるソースファイルに `NamedSource` を紐付けて
/// [`BuildPdfError`] に変換する。
///
/// `origin()` が `Origin::Source` を指す場合はそのファイル名・内容を `NamedSource` に載せた
/// [`BuildPdfError::Lowering`] にし、パースエラーと同じくファイル名・スニペット付きで診断できるようにする。
/// `Origin::Generated`（合成された書誌グループ等）を指す場合は帰属元となる実ソースがないため
/// [`BuildPdfError::LoweringInternal`] にする。span は各ファイル内のオフセットのまま使える
/// （各 `NamedSource` はその 1 ファイル分の `content` だけを持つため、グローバル変換は不要）。
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

  /// グループ 1 に、指定した起源を持たせたうえで未定義ラベルの `\ref` を置き、その起源が帰属源になる
  /// `LoweringError` を生成するテストヘルパ
  ///
  /// `origin` を呼び出し側が指定できる（＝グループの位置と起源が独立している）ことが、
  /// 「配列範囲外の `SourceId`」という暗黙の sentinel をやめて `Origin` を導入した狙いそのもの。
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
    // Arrange — 合成グループ（書誌）に帰属する LoweringError を生成する。実ソース配列の範囲外
    // インデックスに頼らず、Origin::Generated を直接割り当てられることを確認する。
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
