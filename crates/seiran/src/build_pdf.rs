//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

mod error;
mod front_matter;
mod outline;
mod running;

#[cfg(test)]
mod golden;

use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  time::Instant,
};

use document::DocNode;
use error::BuildPdfError;
use font::{
  FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use front_matter::{assemble_front_matter, break_front_matter, page_number_labels};
use lowering::LoweringContext;
use outline::collect_outline_entries;
use parser::ParseSourceError;
use pdf_gen::OutlineEntry;
use running::build_running_spec;
use tracing::{debug, debug_span, info};
use types::AnchorMark;

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
/// 各 source ファイルを順次読み込み、`parser::parse_source` でパース・評価して
/// `Vec<DocNode>` を結合し、1 つのドキュメントとして扱う。
///
/// # エラー戦略
///
/// - I/O 失敗（ファイル読み込み）は早期失敗
/// - パース・評価エラーは全ソースで集約して `MultipleSourceErrors` で報告
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

  let config = read_config::read_config(config_path)?;
  let style = read_style::read_style(config.style_path.as_deref())?;
  let references = read_references::read_references(config.references_path.as_deref())?;

  let stage_start = Instant::now();
  let font_data = FontData::new(&config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  // 描画前パイプライン（パース〜走り文配置）を 1 つの seam に束ねて確定レイアウトを得る。
  let laid_out = build_pages(&config, &style, &references, &font_data)?;

  // 描画は font_refs / metrics を再構築して使う。両者は `font_data` 上のゼロコピービュー
  // （FontRef のパース + head/hhea 参照）で、`build_pages` 内で使ったものは borrow が閉じていて
  // 持ち出せないため描画パス用にもう一度組み直す。フォントファイル自体の再読込は起きない。
  let font_refs = FontRefs::new(&config.font_configs, &font_data)?;
  let metrics = FontMetrics::new(&font_refs)?;

  let stage_start = Instant::now();
  let pdf_bytes =
    pdf_gen::create_pdf(&config, &font_data, &font_refs, &metrics, &laid_out.pages, &style, &laid_out.outline_entries)?;
  info!(page_count = laid_out.pages.len(), elapsed_ms = elapsed_ms(stage_start), "PDF の描画が完了しました");

  let stage_start = Instant::now();
  let output_path = config.output.pdf_path();
  fs::write(&output_path, pdf_bytes).map_err(|source| BuildPdfError::WritePdf {
    path: output_path.display().to_string(),
    source,
  })?;
  info!(output_path = %output_path.display(), elapsed_ms = elapsed_ms(stage_start), "PDF の保存が完了しました");

  return Ok(BuildSummary {
    output_path,
    page_count: laid_out.pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
  });
}

/// [`build_pages`] の出力＝描画パスへ渡す確定レイアウト。
///
/// いずれもフォント非依存の所有データ（[`hlist::Page`] は計測済みグリフ列を持ち `FontRef` を
/// 借用しない、[`OutlineEntry`] はプレーンな見出し情報）なので、フォント関連の借用を伴わずに
/// `build_pages` の外へ持ち出せる。golden スナップショットテストは `pages` をダンプ対象にする。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文を連結した確定ページ列（走り文配置済み）
  pub(super) pages: Vec<hlist::Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(super) outline_entries: Vec<OutlineEntry>,
}

/// 読込済みの設定・スタイル・文献とフォントデータから、描画直前の確定レイアウトを構築する。
///
/// `build_pdf` の描画前パイプライン（ソースのパース → 文献 CSL 整形 → Document IR → `LayoutNode` →
/// `build_blocks` → 画像サイズ確定 → `break_pages` → 走り文配置 → しおり収集）を 1 つの関数に束ねたもの。
/// フォントは `font_data` から内部で `FontRefs` / シェーパー / メトリクスを組み立てて使い、
/// 返り値はフォント非依存の所有データのみ（[`LaidOutDocument`]）。これにより本文組版のロジックを
/// PDF 描画・ファイル I/O から切り離し、確定ページ列を golden テストで直接検証できる。
///
/// # Errors
///
/// ソース読込・パース・文献整形・lowering・フォント検証・段組み幅の不正のいずれかで失敗した場合に
/// エラーを返す。
fn build_pages(
  config: &read_config::Config,
  style: &read_style::Style,
  references: &read_references::References,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  // `\cite` のキー存在検証に使う有効な参照 ID 集合（CSL 整形そのものは後続の citation ステージで実施）
  let citation_keys: HashSet<String> = references.keys().cloned().collect();

  let stage_start = Instant::now();
  let mut doc_nodes = parse_all_sources(&config.sources, style, &citation_keys)?;
  info!(
    source_count = config.sources.len(),
    node_count = doc_nodes.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  // `\cite` を CSL 整形し、引用された文献の書誌を本文末尾に追加する（parser の後・lowering の前）。
  let stage_start = Instant::now();
  citation::process_citations(&mut doc_nodes, references, style)
    .map_err(|source| BuildPdfError::Citation { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "文献引用の CSL 整形が完了しました");

  let stage_start = Instant::now();
  let lowering_ctx = LoweringContext::new(style).with_image_defaults(config.image.max_dpi, config.image.downsample);
  let body_layout_nodes =
    lowering::lower_nodes(&lowering_ctx, &doc_nodes).map_err(|source| BuildPdfError::Lowering { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "Document IR → LayoutNode への変換が完了しました");

  let font_refs = FontRefs::new(&config.font_configs, font_data)?;

  let stage_start = Instant::now();
  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの検証が完了しました");

  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  debug!("シェーパーの初期化が完了しました");

  let metrics = FontMetrics::new(&font_refs)?;

  // 本文幅は画像サイズ解決と行分割の双方で使うので先に算出する
  let text_width = config.pdf.width.to_pt() - config.pdf.margin.left.to_pt() - config.pdf.margin.right.to_pt();
  let default_font_size = style.text.font_size.to_pt();
  let line_height_factor = style.text.line_height_factor;

  // 本文の段組み（前付けは常に単段）。1 段あたりの幅を算出し、非正なら早期にエラーにする
  // （config の用紙・余白 × style の [columns] の横断制約はこのステージでしか検証できない）。
  let body_columns = style.columns.count as usize;
  let column_gap = style.columns.gap.to_pt();
  let body_col_width = hlist::column_width(text_width, body_columns, column_gap);
  if body_col_width <= 0.0 {
    return Err(
      BuildPdfError::InvalidColumnWidth {
        text_width,
        num_columns: body_columns,
        column_gap,
      }
      .into(),
    );
  }

  // build_blocks は本文・タイトルページで複数回呼ばれ、自段完了を同じ文面の DEBUG で出すため、
  // span の `region` で呼び出し区間を区別できるようにする（INFO 時は span 非活性でゼロコスト）。
  let stage_start = Instant::now();
  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    layout::build_blocks(
      body_layout_nodes,
      &harf_rust_shapers,
      &metrics,
      default_font_size,
      line_height_factor,
      config.document.language.as_deref(),
      style.text.punctuation_spacing,
    )
  };
  info!(
    block_count = body_blocks.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文ブロックの構築が完了しました"
  );

  let stage_start = Instant::now();
  // 本文画像は段幅に合わせて解決する（段抜き＝全幅フロートは将来検討）。
  let body_blocks = pdf_gen::resolve_images(body_blocks, body_col_width)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  // ジオメトリは本文（N 段）と前付け（常に 1 段）で分ける。両者は段数・段間以外を共有する。
  let (body_geometry, front_geometry) =
    build_page_geometries(config, style, default_font_size, line_height_factor, body_columns, column_gap);

  // 本文を 1 回だけページ分割する。これがそのまま最終本文ページになる。各見出しの本文内ページ index も
  // ここから採取する。本文は前付け（タイトルページ・目次）と別系列で 1 から番号付けするため、得られる
  // 本文内ページ番号が最終値になる（前付けの長さに不依存 = R1。break_pages は純粋）。
  let stage_start = Instant::now();
  let body_pages = {
    let _span = debug_span!("break_pages", region = "body").entered();
    hlist::break_pages(body_blocks, text_width, &body_geometry, &hlist::KnuthPlassBreaker, style.text.alignment)
  };
  let body_page_count = body_pages.len();
  let heading_pages = heading_page_indices(&body_pages);
  info!(body_page_count, elapsed_ms = elapsed_ms(stage_start), "本文のページ分割が完了しました");

  // 前付けブロック（タイトルページ → 目次）を組み立てる。各リージョンは改ページ境界で始まる。
  // タイトルページのメタデータは config 形状から疎結合にするため本体で構築して渡す。
  let title_metadata = lowering::TitlePageMetadata {
    title: config.document.title.clone(),
    author: config.document.author.clone(),
    date: config.document.date.clone(),
  };
  let front_blocks =
    assemble_front_matter(&doc_nodes, &heading_pages, &title_metadata, style, &harf_rust_shapers, &metrics, text_width);

  // 前付け（1 段）を本文（N 段）と別に分割し、ページ列として連結する。前付けと本文は段数が異なるため
  // 1 回の break_pages では兼ねられない。連結することで本文ページは後ろの index へ自動的にずれ、内部
  // リンク・しおりの参照ページもレンダリング時の列挙で正しく解決される。
  let stage_start = Instant::now();
  let front_pages = {
    let _span = debug_span!("break_pages", region = "front").entered();
    break_front_matter(front_blocks, text_width, &front_geometry, &hlist::KnuthPlassBreaker, style.text.alignment)
  };
  let front_matter_count = front_pages.len();
  let mut pages = front_pages;
  pages.extend(body_pages);
  info!(
    page_count = pages.len(),
    front_matter_count,
    body_page_count,
    elapsed_ms = elapsed_ms(stage_start),
    "ページ分割が完了しました"
  );

  // ページ番号ラベルを算出する（前付け = ローマ数字 / 本文 = 算用数字、各リージョン 1 から）。
  let page_numbers = page_number_labels(pages.len(), front_matter_count, body_page_count, &style.page_numbering);

  // ページ数確定後にヘッダー・フッターを配置する（ページ番号トークンの解決にラベルが必要なため）
  let page_height = config.pdf.height.to_pt();
  let running_spec = build_running_spec(style, &config.document, text_width, page_height, page_numbers);
  layout::build_running_content(&mut pages, &harf_rust_shapers, &metrics, &running_spec);

  // PDF しおり用の見出し情報を文書順に集める（CSL 整形で追加された References 見出しも含む）。
  // lowering が各見出しの直前に出すアンカーと文書順で 1 対 1 に対応する。
  let outline_entries = collect_outline_entries(&doc_nodes);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
  });
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 全 source を順次読み込み、パース結果を結合した `Vec<DocNode>` を返す。
///
/// I/O 失敗は早期にエラーを返し、パース・評価エラーは全 source で集約して
/// [`BuildPdfError::MultipleSourceErrors`] にまとめて返す。
fn parse_all_sources(
  sources: &[std::path::PathBuf],
  style: &read_style::Style,
  citation_keys: &HashSet<String>,
) -> Result<Vec<DocNode>, BuildPdfError> {
  let mut all_nodes: Vec<DocNode> = Vec::new();
  let mut parse_errors: Vec<ParseSourceError> = Vec::new();

  for source_path in sources {
    let content = std::fs::read_to_string(source_path).map_err(|source| BuildPdfError::ReadTextFile {
      path: source_path.display().to_string(),
      source,
    })?;
    let display_path = source_path.display().to_string();
    match parser::parse_source(&content, &display_path, style, citation_keys) {
      Ok(nodes) => all_nodes.extend(nodes),
      Err(error) => parse_errors.push(error),
    }
  }

  if !parse_errors.is_empty() {
    return Err(BuildPdfError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(all_nodes);
}

/// 本文（N 段）と前付け（常に 1 段）の [`hlist::PageGeometry`] を組み立てる。
///
/// 両者は段数・段間以外を共有するため、本文側を組んでから前付けは `num_columns` / `column_gap` だけ
/// 差し替える。
fn build_page_geometries(
  config: &read_config::Config,
  style: &read_style::Style,
  default_font_size: f32,
  line_height_factor: f32,
  body_columns: usize,
  column_gap: f32,
) -> (hlist::PageGeometry, hlist::PageGeometry) {
  let body_geometry = hlist::PageGeometry {
    margin_top: config.pdf.margin.top.to_pt(),
    page_limit: config.pdf.height.to_pt() - config.pdf.margin.bottom.to_pt(),
    default_font_size,
    line_height_factor,
    table_cell_padding: style.table.cell_padding.to_pt(),
    num_columns: body_columns,
    column_gap,
  };
  let front_geometry = hlist::PageGeometry {
    num_columns: 1,
    column_gap: 0.0,
    ..body_geometry
  };
  return (body_geometry, front_geometry);
}

/// 本文 Pass のページ列から、各見出しの本文内ページ index を文書順に採取する。
///
/// `pdf_gen::build_destination_index` と同型のアンカー走査。`document::collect_headings` が返す
/// 見出し列と 1 対 1 に対応する（どちらも文書順）。
fn heading_page_indices(pages: &[hlist::Page]) -> Vec<usize> {
  let mut indices = Vec::new();
  for (page_index, page) in pages.iter().enumerate() {
    for anchor in &page.anchors {
      if matches!(anchor.mark, AnchorMark::Heading { .. }) {
        indices.push(page_index);
      }
    }
  }
  return indices;
}

#[cfg(test)]
mod tests {
  use hlist::{Page, PlacedAnchor};
  use types::AnchorMark;

  use super::heading_page_indices;

  /// 指定マークのアンカーだけを持つページを作るヘルパ
  fn page_with_anchors(marks: Vec<AnchorMark>) -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      anchors: marks
        .into_iter()
        .map(|mark| PlacedAnchor {
          mark,
          x: 0.0,
          y: 0.0,
        })
        .collect(),
      links: Vec::new(),
    };
  }

  #[test]
  fn heading_page_indices_picks_heading_anchors_in_order() {
    // Arrange — page0 に見出し 1 つ、page1 に Label（無視）+ 見出し 1 つ
    let pages = vec![
      page_with_anchors(vec![AnchorMark::Heading {
        key: "heading:0".to_string(),
        label: None,
      }]),
      page_with_anchors(vec![
        AnchorMark::Label("tab:1".to_string()),
        AnchorMark::Heading {
          key: "heading:1".to_string(),
          label: None,
        },
      ]),
    ];

    // Act
    let indices = heading_page_indices(&pages);

    // Assert — 見出しアンカーのページ index だけを文書順に拾う（Label は無視）
    assert_eq!(indices, vec![0, 1]);
  }
}
