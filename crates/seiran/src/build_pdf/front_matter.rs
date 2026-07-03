//! 前付け（タイトルページ・目次）の組み立て・ページ分割・補助型

use document::{DocNode, HeadingLevel, collect_headings, heading_anchor_key, inline_nodes_to_plain_text};
use font::{FontMetrics, shaper::HarfRustShapers};
use hlist::{Block, LineBreaker, Page, PageGeometry};
use layout::{TocEntryInput, TocSpec, build_blocks, build_toc_blocks};
use lowering::{TextStyle, TitlePageMetadata, lower_title_page};
use read_style::{PageNumbering, Style, TocStyle};
use tracing::{debug, debug_span};
use types::{FontKind, TextAlignment};

/// 前付けブロック（タイトルページ → 目次）を文書順に組み立てて返す。
///
/// 各リージョンは改ページ境界で始まる。`lower_title_page` は末尾に `PageBreak` を含むため、後続
/// （目次・本文）は次ページから始まる。目次の後にも `Block::PageBreak` を積み、本文を次ページから
/// 始める（本文区間の独立性を保つ）。`title_page` / `toc` がともに無効なら空列を返す。
pub(super) fn assemble_front_matter(
  doc_nodes: &[DocNode],
  heading_pages: &[usize],
  title_metadata: &TitlePageMetadata,
  style: &Style,
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
  text_width: f32,
) -> Vec<Block> {
  // build_blocks 用の既定サイズは style から導出する（呼び出し本体と同じ値）。
  let default_font_size = style.text.font_size.to_pt();
  let line_height_factor = style.text.line_height_factor;
  let mut front_blocks: Vec<Block> = Vec::new();
  if style.title_page.enabled {
    let title_nodes = lower_title_page(title_metadata, &style.title_page);
    {
      let _span = debug_span!("build_blocks", region = "title").entered();
      // タイトルページ（表題・著者等の大きな見出し文字）はハイフネーションしない（#173）
      front_blocks.extend(build_blocks(title_nodes, shapers, metrics, default_font_size, line_height_factor, None));
    }
    debug!("タイトルページを生成しました");
  }
  if style.toc.enabled {
    let toc_entries = collect_toc_entries(doc_nodes, heading_pages, &style.toc, &style.page_numbering);
    let toc_spec = build_toc_spec(style, text_width);
    let toc_blocks = build_toc_blocks(&toc_spec, &toc_entries, shapers, metrics);
    if !toc_blocks.is_empty() {
      front_blocks.extend(toc_blocks);
      front_blocks.push(Block::PageBreak);
      debug!(toc_entry_count = toc_entries.len(), "目次を生成しました");
    }
  }
  return front_blocks;
}

/// 見出しと本文内ページ index から目次エントリ列を組み立てる。
///
/// `max_depth` を超える深さの見出しは除外する。ページラベルは本文の番号スタイル（算用数字）で
/// レンダリングし、内部リンクキーは見出しの文書順インデックスから [`document::heading_anchor_key`]
/// で得る（lowering 側の `AnchorMark::Heading.key` と一致する）。
fn collect_toc_entries(
  doc_nodes: &[DocNode],
  heading_pages: &[usize],
  toc: &TocStyle,
  page_numbering: &PageNumbering,
) -> Vec<TocEntryInput> {
  let headings = collect_headings(doc_nodes);
  debug_assert_eq!(headings.len(), heading_pages.len(), "見出し数と採取したページ数は一致するはず");
  return headings
    .into_iter()
    .zip(heading_pages.iter().copied())
    .filter(|(info, _)| u32::from(info.level.depth()) < toc.max_depth)
    .map(|(info, page_index)| TocEntryInput {
      level: info.level,
      number: info.number.to_string(),
      title_plain: inline_nodes_to_plain_text(info.title),
      page_label: page_numbering.body.render(page_index as u32 + 1),
      link_key: heading_anchor_key(info.index),
    })
    .collect();
}

/// `style.toc` と本文スタイルから目次生成用の [`layout::TocSpec`] を組み立てる。
///
/// 目次見出しの書体は文書の節見出しスタイル（[`document::HeadingLevel::Section`]）に揃える。
fn build_toc_spec(style: &Style, text_width: f32) -> TocSpec {
  let toc = &style.toc;
  let title_heading = style.heading(HeadingLevel::Section);
  return TocSpec {
    title: toc.title.clone(),
    title_style: TextStyle {
      font_size: title_heading.font_size.to_pt(),
      font_kind: title_heading.font_kind,
      color: None,
    },
    title_bottom_margin: title_heading.bottom_margin.to_pt(),
    entry_style: TextStyle {
      font_size: toc.font_size.to_pt(),
      font_kind: FontKind::Serif,
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

/// 前付け（タイトルページ → 目次）ブロックを単独でページ分割する。
///
/// 前付けは常に単段（`front_geometry`）。本文ページ列と連結する前提なので、本文との区切り用に末尾へ
/// 付いている `Block::PageBreak` は落とす（[`hlist::break_pages`] の `finish` が末尾ページを無条件に
/// push するため、残すと空の末尾ページが生じる）。タイトル → 目次間の中間 `PageBreak` は保持する。
/// 前付けが空（タイトルページ・目次ともに無効）のときは空ページを作らず空の列を返す。
pub(super) fn break_front_matter(
  mut front_blocks: Vec<Block>,
  text_width: f32,
  front_geometry: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
) -> Vec<Page> {
  if matches!(front_blocks.last(), Some(Block::PageBreak)) {
    front_blocks.pop();
  }
  if front_blocks.is_empty() {
    return Vec::new();
  }
  return hlist::break_pages(front_blocks, text_width, front_geometry, breaker, alignment);
}

/// 各物理ページの `(\{page\}, \{pages\})` ラベルをリージョン別に算出する。
///
/// 前付け（index `< front_count`）は `page_numbering.front_matter`（既定ローマ数字）で 1 から、
/// 本文はそれ以降を `page_numbering.body`（既定算用数字）で 1 から振り直す。`\{pages\}` は同じ
/// リージョンの総数を同じスタイルでレンダリングしたもの。
pub(super) fn page_number_labels(
  total: usize,
  front_count: usize,
  body_count: usize,
  page_numbering: &PageNumbering,
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

#[cfg(test)]
mod tests {
  use document::{DocNode, HeadingLevel, InlineNode};
  use read_style::{PageNumbering, TocStyle};

  use super::{collect_toc_entries, page_number_labels};

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
