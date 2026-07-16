//! 前付け（タイトルページ・目次）の組み立て・ページ分割・補助型

use config::{Style, TocStyle};
use font::{FontMetrics, shaper::HarfRustShapers};
use model::{Block, FontKind, HeadingLevel, Page, TextAlignment, heading_anchor_key};
use tracing::{debug, debug_span};
use typeset::{
  HeadingRecord, LineBreaker, PageGeometry, TextStyle, TitlePageMetadata, TocEntryInput, TocSpec, build_blocks,
  build_toc_blocks, lower_title_page,
};

use super::page_values::BodyPageValues;

/// 前付けブロック（タイトルページ → 目次）を文書順に組み立てて返す。
///
/// 各リージョンは改ページ境界で始まる。`lower_title_page` は末尾に `PageBreak` を含むため、後続
/// （目次・本文）は次ページから始まる。目次の後にも `Block::PageBreak` を積み、本文を次ページから
/// 始める（本文区間の独立性を保つ）。`title_page` / `toc` がともに無効なら空列を返す。
pub(super) fn assemble_front_matter(
  headings: &[HeadingRecord],
  page_values: &BodyPageValues,
  title_metadata: &TitlePageMetadata,
  style: &Style,
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
  text_width: model::Length,
) -> Vec<Block> {
  // build_blocks 用の既定サイズは style から導出する（呼び出し本体と同じ値）。
  let default_font_size = style.text.font_size;
  let line_height_factor = style.text.line_height_factor;
  let mut front_blocks: Vec<Block> = Vec::new();
  if style.title_page.enabled {
    let title_nodes = lower_title_page(title_metadata, &style.title_page);
    {
      let _span = debug_span!("build_blocks", region = "title").entered();
      // タイトルページ（表題・著者等の大きな見出し文字）はハイフネーションしない（#173）
      front_blocks.extend(build_blocks(
        title_nodes,
        shapers,
        metrics,
        default_font_size,
        line_height_factor,
        None,
        style.text.punctuation_spacing,
      ));
    }
    debug!("タイトルページを生成しました");
  }
  if style.toc.enabled {
    let toc_entries = collect_toc_entries(headings, page_values, &style.toc);
    let toc_spec = build_toc_spec(style, text_width);
    let toc_blocks = build_toc_blocks(&toc_spec, &toc_entries, shapers, metrics);
    if !toc_blocks.is_empty() {
      front_blocks.extend(toc_blocks);
      front_blocks.push(Block::force_break());
      debug!(toc_entry_count = toc_entries.len(), "目次を生成しました");
    }
  }
  return front_blocks;
}

/// 見出しと本文内ページ index から目次エントリ列を組み立てる。
///
/// `max_depth` を超える深さの見出しは除外する。ページラベルは [`BodyPageValues::body_page_label`]
/// （本文の番号スタイル＝算用数字）でレンダリングし、内部リンクキーは見出しの文書順インデックスから
/// [`model::heading_anchor_key`] で得る（lowering 側の `AnchorMark::Heading.key` と一致する）。
fn collect_toc_entries(headings: &[HeadingRecord], page_values: &BodyPageValues, toc: &TocStyle) -> Vec<TocEntryInput> {
  let heading_pages = page_values.heading_pages();
  debug_assert_eq!(headings.len(), heading_pages.len(), "見出し数と採取したページ数は一致するはず");
  return headings
    .iter()
    .zip(heading_pages.iter().copied())
    .filter(|(info, _)| u32::from(info.level.depth()) < toc.max_depth)
    .map(|(info, page_index)| TocEntryInput {
      level: info.level,
      number: info.number.clone(),
      title_plain: info.title_plain.clone(),
      page_label: page_values.body_page_label(page_index),
      link_key: heading_anchor_key(info.index),
    })
    .collect();
}

/// `style.toc` と本文スタイルから目次生成用の [`typeset::TocSpec`] を組み立てる。
///
/// 目次見出しの書体は文書の節見出しスタイル（[`model::HeadingLevel::Section`]）に揃える。
fn build_toc_spec(style: &Style, text_width: model::Length) -> TocSpec {
  let toc = &style.toc;
  let title_heading = style.heading(HeadingLevel::Section);
  return TocSpec {
    title: toc.title.clone(),
    title_style: TextStyle {
      font_size: title_heading.font_size,
      font_kind: title_heading.font_kind,
      color: None,
    },
    title_bottom_margin: title_heading.bottom_margin,
    entry_style: TextStyle {
      font_size: toc.font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
    indent_per_level: toc.indent_per_level,
    leader: toc.leader.clone(),
    show_page_numbers: toc.show_page_numbers,
    text_width,
    line_height_factor: style.text.line_height_factor,
    bottom_margin: toc.bottom_margin,
  };
}

/// 前付け（タイトルページ → 目次）ブロックを単独でページ分割する。
///
/// 前付けは常に単段（`front_geometry`）。本文ページ列と連結する前提なので、本文との区切り用に末尾へ
/// 付いている強制改ページ（[`Block::force_break`]）は落とす（[`typeset::break_pages`] の `finish` が末尾
/// ページを無条件に push するため、残すと空の末尾ページが生じる）。タイトル → 目次間の中間の強制改ページは保持する。
/// 前付けが空（タイトルページ・目次ともに無効）のときは空ページを作らず空の列を返す。
pub(super) fn break_front_matter(
  mut front_blocks: Vec<Block>,
  text_width: model::Length,
  front_geometry: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
) -> Vec<Page> {
  if front_blocks.last().is_some_and(Block::is_force_break) {
    front_blocks.pop();
  }
  if front_blocks.is_empty() {
    return Vec::new();
  }
  return typeset::break_pages(front_blocks, text_width, front_geometry, breaker, alignment);
}

#[cfg(test)]
mod tests {
  use config::{PageNumbering, TocStyle};
  use model::{AnchorMark, HeadingLevel, PlacedAnchor};
  use typeset::HeadingRecord;

  use super::{BodyPageValues, Page, collect_toc_entries};

  fn heading_record(index: usize, level: HeadingLevel, number: &str, title_plain: &str) -> HeadingRecord {
    return HeadingRecord {
      index,
      level,
      number: number.to_string(),
      title_plain: title_plain.to_string(),
    };
  }

  /// 各ページに 1 つずつ見出しアンカーを持つ本文ページ列から [`BodyPageValues`] を作るヘルパ
  fn body_page_values_with_headings(heading_count: usize) -> BodyPageValues {
    let pages: Vec<Page> = (0..heading_count)
      .map(|index| Page {
        blocks: Vec::new(),
        header: Vec::new(),
        footer: Vec::new(),
        footnotes: Vec::new(),
        anchors: vec![PlacedAnchor {
          mark: AnchorMark::Heading {
            key: format!("heading:{index}"),
            label: None,
          },
          x: model::Length::ZERO,
          y: model::Length::ZERO,
        }],
        links: Vec::new(),
      })
      .collect();
    return BodyPageValues::from_body_pages(&pages, &PageNumbering::default());
  }

  #[test]
  fn collect_toc_entries_filters_by_max_depth_and_renders_page_label() {
    // Arrange — Chapter(深さ1)/Section(深さ2)/Subsection(深さ3)。max_depth=3 は深さ<3 を残す
    let headings = vec![
      heading_record(0, HeadingLevel::Chapter, "1", "Ch"),
      heading_record(1, HeadingLevel::Section, "1.1", "Sec"),
      heading_record(2, HeadingLevel::Subsection, "1.1.1", "Sub"),
    ];
    let page_values = body_page_values_with_headings(3);
    let toc = TocStyle {
      max_depth: 3,
      ..TocStyle::default()
    };

    // Act
    let entries = collect_toc_entries(&headings, &page_values, &toc);

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
