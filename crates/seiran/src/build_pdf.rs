//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  time::Instant,
};

use citation::CitationError;
use document::DocNode;
use font::{
  FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use lowering::{LoweringContext, LoweringError};
use miette::Diagnostic;
use parser::ParseSourceError;
use thiserror::Error;
use tracing::{debug, debug_span, info};

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

/// PDF ビルド時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum BuildPdfError {
  /// テキストファイルの読み込みに失敗した場合
  #[error("テキストファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(build::read_text_file),
    help(
      "ファイルのパスと読み取り権限を確認してください。ファイルが UTF-8 でエンコードされていることも確認してください。"
    )
  )]
  ReadTextFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// 複数ソースのパース・評価で発生したエラーの集約
  ///
  /// 補足設計のエラー戦略: 文法・評価エラーは集約して 1 度に報告する。
  /// 各 `ParseSourceError` は `NamedSource` と内側エラーの label を保持しているため、
  /// `#[related]` 経由で `miette` のフル診断（ソースコード付き）が表示される。
  #[error("複数のソースファイルでエラーが発生しました。")]
  #[diagnostic(code(build::multiple_source_errors))]
  MultipleSourceErrors {
    #[related]
    errors: Vec<ParseSourceError>,
  },

  /// 文献引用（`\cite`）の CSL 整形ステージで発生したエラー
  ///
  /// 内側の [`CitationError`] が持つ `code` / `help` は `#[diagnostic_source]` により外側へ伝播される。
  #[error("文献引用の整形に失敗しました。")]
  #[diagnostic(code(build::citation))]
  Citation {
    /// 元の citation エラー
    #[source]
    #[diagnostic_source]
    source: CitationError,
  },

  /// Document IR → `LayoutNode` 変換（lowering）で発生したエラー
  ///
  /// 内側の [`LoweringError`] が持つ `code` / `help` は `#[diagnostic_source]` により
  /// 外側へ伝播されます。
  #[error("ドキュメントのレイアウト変換に失敗しました。")]
  #[diagnostic(code(build::lowering))]
  Lowering {
    /// 元の lowering エラー
    #[source]
    #[diagnostic_source]
    source: LoweringError,
  },

  /// PDF ファイルの書き込みに失敗した場合
  #[error("PDF ファイルの保存に失敗しました: {path}")]
  #[diagnostic(code(build::write_pdf), help("出力ディレクトリが存在し、書き込み権限があることを確認してください。"))]
  WritePdf {
    /// 出力パス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
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
  // `\cite` のキー存在検証に使う有効な参照 ID 集合（CSL 整形そのものは後続の citation ステージで実施）
  let citation_keys: HashSet<String> = references.keys().cloned().collect();

  let stage_start = Instant::now();
  let mut doc_nodes = parse_all_sources(&config.sources, &style, &citation_keys)?;
  info!(
    source_count = config.sources.len(),
    node_count = doc_nodes.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  // `\cite` を CSL 整形し、引用された文献の書誌を本文末尾に追加する（parser の後・lowering の前）。
  let stage_start = Instant::now();
  citation::process_citations(&mut doc_nodes, &references, &style)
    .map_err(|source| BuildPdfError::Citation { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "文献引用の CSL 整形が完了しました");

  let stage_start = Instant::now();
  let lowering_ctx = LoweringContext::new(&style).with_image_defaults(config.image.max_dpi, config.image.downsample);
  let body_layout_nodes =
    lowering::lower_nodes(&lowering_ctx, &doc_nodes).map_err(|source| BuildPdfError::Lowering { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "Document IR → LayoutNode への変換が完了しました");

  let stage_start = Instant::now();
  let font_data = FontData::new(&config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  let font_refs = FontRefs::new(&config.font_configs, &font_data)?;

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

  // build_blocks は本文・タイトルページで複数回呼ばれ、自段完了を同じ文面の DEBUG で出すため、
  // span の `region` で呼び出し区間を区別できるようにする（INFO 時は span 非活性でゼロコスト）。
  let stage_start = Instant::now();
  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    layout::build_blocks(body_layout_nodes, &harf_rust_shapers, &metrics, default_font_size, line_height_factor)
  };
  info!(
    block_count = body_blocks.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文ブロックの構築が完了しました"
  );

  let stage_start = Instant::now();
  let body_blocks = pdf_gen::resolve_images(body_blocks, text_width)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  let geometry = hlist::PageGeometry {
    margin_top: config.pdf.margin.top.to_pt(),
    page_limit: config.pdf.height.to_pt() - config.pdf.margin.bottom.to_pt(),
    default_font_size,
    line_height_factor,
    table_cell_padding: style.table.cell_padding.to_pt(),
  };

  // Pass 1: 本文だけを単独でページ分割し、各見出しの本文内ページ index を採取する。
  // 本文は前付け（タイトルページ・目次）と別系列で 1 から番号付けするため、ここで得る本文内
  // ページ番号が最終値になる（前付けの長さに不依存 = R1。break_pages は純粋なので安価）。
  // break_pages は Pass 1 / Pass 2 で 2 回呼ばれ、自段完了を同じ文面の DEBUG で出すため、
  // span の `pass` でどちらの呼び出しかを区別できるようにする。
  let stage_start = Instant::now();
  let body_pages = {
    let _span = debug_span!("break_pages", pass = 1).entered();
    hlist::break_pages(body_blocks.clone(), text_width, &geometry, &hlist::GreedyBreaker)
  };
  let body_page_count = body_pages.len();
  let heading_pages = heading_page_indices(&body_pages);
  info!(body_page_count, elapsed_ms = elapsed_ms(stage_start), "本文のページ分割が完了しました（Pass 1）");

  // 前付けブロック（タイトルページ → 目次）を組み立てる。各リージョンは改ページ境界で始まる。
  let mut front_blocks: Vec<hlist::Block> = Vec::new();
  if style.title_page.enabled {
    let metadata = lowering::TitlePageMetadata {
      title: config.document.title.clone(),
      author: config.document.author.clone(),
      date: config.document.date.clone(),
    };
    // lower_title_page は末尾に PageBreak を含むため、後続（目次・本文）は次ページから始まる。
    let title_nodes = lowering::lower_title_page(&metadata, &style.title_page);
    {
      let _span = debug_span!("build_blocks", region = "title").entered();
      front_blocks.extend(layout::build_blocks(
        title_nodes,
        &harf_rust_shapers,
        &metrics,
        default_font_size,
        line_height_factor,
      ));
    }
    debug!("タイトルページを生成しました");
  }
  if style.toc.enabled {
    let toc_entries = collect_toc_entries(&doc_nodes, &heading_pages, &style.toc, &style.page_numbering);
    let toc_spec = build_toc_spec(&style, text_width);
    let toc_blocks = layout::build_toc_blocks(&toc_spec, &toc_entries, &harf_rust_shapers, &metrics);
    if !toc_blocks.is_empty() {
      front_blocks.extend(toc_blocks);
      // 目次の後で改ページし、本文を次ページから始める（本文区間の独立性を保つ）。
      front_blocks.push(hlist::Block::PageBreak);
      debug!(toc_entry_count = toc_entries.len(), "目次を生成しました");
    }
  }

  // Pass 2: 前付け + 本文を結合して最終ページを確定する。本文区間のページ分割は Pass 1 と
  // 一致する（本文は常に改ページ境界から始まるため）。
  let stage_start = Instant::now();
  let mut combined = front_blocks;
  combined.extend(body_blocks);
  let mut pages = {
    let _span = debug_span!("break_pages", pass = 2).entered();
    hlist::break_pages(combined, text_width, &geometry, &hlist::GreedyBreaker)
  };
  let front_matter_count = pages.len().saturating_sub(body_page_count);
  debug_assert_eq!(pages.len(), front_matter_count + body_page_count, "本文区間のページ数は Pass 1 と一致するはず");
  info!(
    page_count = pages.len(),
    front_matter_count,
    body_page_count,
    elapsed_ms = elapsed_ms(stage_start),
    "ページ分割が完了しました（Pass 2）"
  );

  // ページ番号ラベルを算出する（前付け = ローマ数字 / 本文 = 算用数字、各リージョン 1 から）。
  let page_numbers = page_number_labels(pages.len(), front_matter_count, body_page_count, &style.page_numbering);

  // ページ数確定後にヘッダー・フッターを配置する（ページ番号トークンの解決にラベルが必要なため）
  let page_height = config.pdf.height.to_pt();
  let running_spec = layout::RunningContentSpec {
    header: running_slots(&style.header, style.header.baseline_offset.to_pt(), true),
    footer: running_slots(&style.footer, page_height - style.footer.baseline_offset.to_pt(), false),
    metadata: layout::RunningMetadata {
      title: config.document.title.clone().unwrap_or_default(),
      author: config.document.author.clone().unwrap_or_default(),
      date: config.document.date.clone().unwrap_or_default(),
    },
    text_width,
    page_numbers,
    // タイトルページ（先頭ページ）にはヘッダー・フッターを描画しない
    skip_first: style.title_page.enabled,
  };
  layout::build_running_content(&mut pages, &harf_rust_shapers, &metrics, &running_spec);

  // PDF しおり用の見出し情報を文書順に集める（CSL 整形で追加された References 見出しも含む）。
  // lowering が各見出しの直前に出すアンカーと文書順で 1 対 1 に対応する。
  let outline_entries = collect_outline_entries(&doc_nodes);

  let stage_start = Instant::now();
  let pdf_bytes = pdf_gen::create_pdf(&config, &font_data, &font_refs, &metrics, &pages, &style, &outline_entries)?;
  info!(page_count = pages.len(), elapsed_ms = elapsed_ms(stage_start), "PDF の描画が完了しました");

  let stage_start = Instant::now();
  let output_path = config.output.pdf_path();
  fs::write(&output_path, pdf_bytes).map_err(|source| BuildPdfError::WritePdf {
    path: output_path.display().to_string(),
    source,
  })?;
  info!(output_path = %output_path.display(), elapsed_ms = elapsed_ms(stage_start), "PDF の保存が完了しました");

  return Ok(BuildSummary {
    output_path,
    page_count: pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
  });
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// Document IR の見出しから PDF しおり用の [`pdf_gen::OutlineEntry`] を文書順に組み立てる。
///
/// テキストは `"{number} {plain title}"`（番号が空なら表題のみ）。見出しの収集は意味側の
/// 単一ソース [`document::collect_headings`] に委譲する（目次生成と同じソースを使う）。
fn collect_outline_entries(doc_nodes: &[DocNode]) -> Vec<pdf_gen::OutlineEntry> {
  return document::collect_headings(doc_nodes)
    .into_iter()
    .map(|info| {
      let plain = document::inline_nodes_to_plain_text(info.title);
      let text = heading_label(info.number, &plain);
      return pdf_gen::OutlineEntry {
        level: info.level,
        text,
      };
    })
    .collect();
}

/// 「番号 タイトル」の表示文字列を組む（番号・タイトルの空を考慮）。しおり・目次で共用する。
fn heading_label(number: &str, title_plain: &str) -> String {
  if number.is_empty() {
    return title_plain.to_string();
  }
  if title_plain.is_empty() {
    return number.to_string();
  }
  return format!("{number} {title_plain}");
}

/// 本文 Pass のページ列から、各見出しの本文内ページ index を文書順に採取する。
///
/// `pdf_gen::build_destination_index` と同型のアンカー走査。`document::collect_headings` が返す
/// 見出し列と 1 対 1 に対応する（どちらも文書順）。
fn heading_page_indices(pages: &[hlist::Page]) -> Vec<usize> {
  let mut indices = Vec::new();
  for (page_index, page) in pages.iter().enumerate() {
    for anchor in &page.anchors {
      if matches!(anchor.mark, types::AnchorMark::Heading { .. }) {
        indices.push(page_index);
      }
    }
  }
  return indices;
}

/// 見出しと本文内ページ index から目次エントリ列を組み立てる。
///
/// `max_depth` を超える深さの見出しは除外する。ページラベルは本文の番号スタイル（算用数字）で
/// レンダリングし、内部リンクキーは見出しの文書順インデックスから [`document::heading_anchor_key`]
/// で得る（lowering 側の `AnchorMark::Heading.key` と一致する）。
fn collect_toc_entries(
  doc_nodes: &[DocNode],
  heading_pages: &[usize],
  toc: &read_style::TocStyle,
  page_numbering: &read_style::PageNumbering,
) -> Vec<layout::TocEntryInput> {
  let headings = document::collect_headings(doc_nodes);
  debug_assert_eq!(headings.len(), heading_pages.len(), "見出し数と採取したページ数は一致するはず");
  return headings
    .into_iter()
    .zip(heading_pages.iter().copied())
    .filter(|(info, _)| u32::from(info.level.depth()) < toc.max_depth)
    .map(|(info, page_index)| layout::TocEntryInput {
      level: info.level,
      number: info.number.to_string(),
      title_plain: document::inline_nodes_to_plain_text(info.title),
      page_label: page_numbering.body.render(page_index as u32 + 1),
      link_key: document::heading_anchor_key(info.index),
    })
    .collect();
}

/// `style.toc` と本文スタイルから目次生成用の [`layout::TocSpec`] を組み立てる。
///
/// 目次見出しの書体は文書の節見出しスタイル（[`document::HeadingLevel::Section`]）に揃える。
fn build_toc_spec(style: &read_style::Style, text_width: f32) -> layout::TocSpec {
  let toc = &style.toc;
  let title_heading = style.heading(document::HeadingLevel::Section);
  return layout::TocSpec {
    title: toc.title.clone(),
    title_style: lowering::TextStyle {
      font_size: title_heading.font_size.to_pt(),
      font_kind: title_heading.font_kind,
      color: None,
    },
    title_bottom_margin: title_heading.bottom_margin.to_pt(),
    entry_style: lowering::TextStyle {
      font_size: toc.font_size.to_pt(),
      font_kind: types::FontKind::Serif,
      color: None,
    },
    indent_per_level: toc.indent_per_level.to_pt(),
    leader: toc.leader.clone(),
    show_page_numbers: toc.show_page_numbers,
    text_width,
    line_height_factor: style.text.line_height_factor,
    bottom_margin: toc.bottom_margin.to_pt(),
  };
}

/// 各物理ページの `(\{page\}, \{pages\})` ラベルをリージョン別に算出する。
///
/// 前付け（index `< front_count`）は `page_numbering.front_matter`（既定ローマ数字）で 1 から、
/// 本文はそれ以降を `page_numbering.body`（既定算用数字）で 1 から振り直す。`\{pages\}` は同じ
/// リージョンの総数を同じスタイルでレンダリングしたもの。
fn page_number_labels(
  total: usize,
  front_count: usize,
  body_count: usize,
  page_numbering: &read_style::PageNumbering,
) -> Vec<(String, String)> {
  let mut labels = Vec::with_capacity(total);
  for index in 0..total {
    if index < front_count {
      let page = page_numbering.front_matter.render(index as u32 + 1);
      let pages = page_numbering.front_matter.render(front_count as u32);
      labels.push((page, pages));
    } else {
      let body_index = index - front_count;
      let page = page_numbering.body.render(body_index as u32 + 1);
      let pages = page_numbering.body.render(body_count as u32);
      labels.push((page, pages));
    }
  }
  return labels;
}

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

/// `RunningContentStyle` をヘッダー・フッター配置用の [`layout::RunningSlots`] に変換する。
///
/// 全スロットが空のリージョンは描画不要なので `None` を返し、配置パスを省略させる。
/// `baseline_y` はベースラインのページ上端からの絶対距離（フッターは呼び出し側で換算済み）、
/// `rule_below` は区切り線をテキストの下に置くか（ヘッダーは `true`、フッターは `false`）。
fn running_slots(
  style: &read_style::RunningContentStyle,
  baseline_y: f32,
  rule_below: bool,
) -> Option<layout::RunningSlots> {
  if style.is_empty() {
    return None;
  }
  return Some(layout::RunningSlots {
    left: style.left.clone(),
    center: style.center.clone(),
    right: style.right.clone(),
    font_kind: style.font_kind,
    font_size: style.font_size.to_pt(),
    baseline_y,
    rule_below,
    rule_thickness: style.rule_thickness.to_pt(),
    rule_gap: style.rule_gap.to_pt(),
    rule_color: style.rule_color.map(types::Color::rgb),
  });
}

#[cfg(test)]
mod tests {
  use document::{DocNode, HeadingLevel, InlineNode};
  use hlist::{Page, PlacedAnchor};
  use read_style::{PageNumbering, TocStyle};
  use types::AnchorMark;

  use super::{collect_toc_entries, heading_label, heading_page_indices, page_number_labels};

  #[test]
  fn heading_label_combines_number_and_title() {
    assert_eq!(heading_label("1.2", "Intro"), "1.2 Intro");
    assert_eq!(heading_label("", "Intro"), "Intro");
    assert_eq!(heading_label("1.2", ""), "1.2");
  }

  #[test]
  fn page_number_labels_roman_front_arabic_body() {
    // Arrange — 既定（前付け=ローマ小文字 / 本文=算用）。total=5, front=2, body=3
    let pn = PageNumbering::default();

    // Act
    let labels = page_number_labels(5, 2, 3, &pn);

    // Assert — 前付けは i, ii（総数 ii）、本文は 1..3（総数 3）でリージョン別に振り直す
    assert_eq!(labels[0], ("i".to_string(), "ii".to_string()));
    assert_eq!(labels[1], ("ii".to_string(), "ii".to_string()));
    assert_eq!(labels[2], ("1".to_string(), "3".to_string()));
    assert_eq!(labels[4], ("3".to_string(), "3".to_string()));
  }

  #[test]
  fn page_number_labels_without_front_matter_is_plain_arabic() {
    // 前付けが無ければ全ページが本文系列（算用数字 1 から）= 従来挙動
    let labels = page_number_labels(3, 0, 3, &PageNumbering::default());
    assert_eq!(labels[0].0, "1");
    assert_eq!(labels[2], ("3".to_string(), "3".to_string()));
  }

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

  #[test]
  fn collect_toc_entries_filters_by_max_depth_and_renders_page_label() {
    // Arrange — Chapter(深さ1)/Section(深さ2)/Subsection(深さ3)。max_depth=3 は深さ<3 を残す
    let doc = vec![
      DocNode::heading(HeadingLevel::Chapter, "1", vec![InlineNode::text("Ch")]),
      DocNode::heading(HeadingLevel::Section, "1.1", vec![InlineNode::text("Sec")]),
      DocNode::heading(HeadingLevel::Subsection, "1.1.1", vec![InlineNode::text("Sub")]),
    ];
    let heading_pages = vec![0, 1, 2];
    let toc = TocStyle {
      max_depth: 3,
      ..TocStyle::default()
    };

    // Act
    let entries = collect_toc_entries(&doc, &heading_pages, &toc, &PageNumbering::default());

    // Assert — Subsection は除外、ページラベルは本文算用数字、リンクキーは文書順インデックス由来
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].number, "1");
    assert_eq!(entries[0].page_label, "1");
    assert_eq!(entries[0].link_key, "heading:0");
    assert_eq!(entries[1].title_plain, "Sec");
    assert_eq!(entries[1].page_label, "2");
    assert_eq!(entries[1].link_key, "heading:1");
  }
}
