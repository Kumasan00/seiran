//! 前付け（タイトルページ・目次）の組み立て・ページ分割・補助型

use std::time::Instant;

use config::{Style, TocStyle};
use font::FontSystem;
use model::{FontKind, HeadingKey, HeadingLevel, TextAlignment};
use tracing::{debug, debug_span, info};
use typeset::{
  Block, HeadingRecord, LineBreaker, Page, PageGeometry, TextStyle, TitlePageMetadata, TocEntryInput, TocSpec,
  build_blocks, build_toc_blocks, lower_title_page,
};

use super::{
  elapsed_ms,
  page_values::BodyPageValues,
  phase_context::{BodyPageFacts, CompileContext},
};

/// 前付け（タイトルページ・目次）を生成してページ分割する。
///
/// タイトルページのメタデータは config 形状から疎結合にするためここで組み立てる。前付けは常に
/// 1 段組み（`front_geometry`）で、本文（N 段）とは別に分割する。
pub(super) fn typeset_front_matter(ctx: &CompileContext<'_>, facts: &BodyPageFacts) -> Vec<Page> {
  let stage_start = Instant::now();
  let title_metadata = TitlePageMetadata {
    title: ctx.config.document.title.clone(),
    author: ctx.config.document.author.clone(),
    date: ctx.config.document.date.clone(),
  };
  let front_blocks = assemble_front_matter(
    &facts.headings,
    &facts.page_values,
    &title_metadata,
    ctx.style,
    ctx.resources,
    ctx.text_width,
  );
  let pages = {
    let _span = debug_span!("break_pages", region = "front").entered();
    break_front_matter(
      front_blocks,
      ctx.text_width,
      &ctx.front_geometry,
      &typeset::KnuthPlassBreaker,
      ctx.style.text.alignment,
    )
  };
  info!(
    front_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "前付けのページ分割が完了しました"
  );
  return pages;
}

/// 前付けブロック（タイトルページ → 目次）を文書順に組み立てて返す。
///
/// 各リージョンは改ページ境界で始まる。`lower_title_page` は末尾に `PageBreak` を含むため、後続
/// （目次・本文）は次ページから始まる。目次の後にも `Block::PageBreak` を積み、本文を次ページから
/// 始める（本文区間の独立性を保つ）。`title_page` / `toc` がともに無効なら空列を返す。
fn assemble_front_matter(
  headings: &[HeadingRecord],
  page_values: &BodyPageValues,
  title_metadata: &TitlePageMetadata,
  style: &Style,
  resources: &FontSystem<'_>,
  text_width: model::Length,
) -> Vec<Block> {
  let default_font_size = style.text.font_size;
  let line_height_factor = style.text.line_height_factor;
  let mut front_blocks: Vec<Block> = Vec::new();
  if style.title_page.enabled {
    let title_nodes = lower_title_page(title_metadata, &style.title_page);
    {
      let _span = debug_span!("build_blocks", region = "title").entered();
      // タイトルページはハイフネーションしない
      front_blocks.extend(build_blocks(
        title_nodes,
        resources,
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
    let toc_blocks = build_toc_blocks(&toc_spec, &toc_entries, resources);
    if !toc_blocks.is_empty() {
      front_blocks.extend(toc_blocks);
      front_blocks.push(Block::force_break());
      debug!(toc_entry_count = toc_entries.len(), "目次を生成しました");
    }
  }
  return front_blocks;
}

/// 見出しと本文内ページ index から目次エントリを組み立てる。
///
/// `max_depth` 以上の見出しは除外し、本文の番号スタイルでページラベルを作る。
fn collect_toc_entries(headings: &[HeadingRecord], page_values: &BodyPageValues, toc: &TocStyle) -> Vec<TocEntryInput> {
  let heading_pages = page_values.heading_pages();
  debug_assert_eq!(headings.len(), heading_pages.len(), "見出し数と採取したページ数は一致するはず");
  return headings
    .iter()
    .zip(heading_pages.iter().copied())
    .filter(|(info, _)| return u32::from(info.level.depth()) < toc.max_depth)
    .map(|(info, page_index)| {
      return TocEntryInput {
        level: info.level,
        number: info.number.clone(),
        title_plain: info.title_plain.clone(),
        page_label: page_values.body_page_label(page_index),
        link_key: HeadingKey::new(info.index),
      };
    })
    .collect();
}

/// スタイルから目次生成用の [`typeset::TocSpec`] を組み立てる。
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

/// 前付けブロックを単独でページ分割する。
///
/// 本文との境界用に末尾へ置いた強制改ページは、空ページを作らないよう分割前に除く。
fn break_front_matter(
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
  use model::{AnchorMark, HeadingKey, HeadingLevel};
  use typeset::{HeadingRecord, PlacedAnchor};

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
      .map(|index| {
        return Page {
          blocks: Vec::new(),
          header: Vec::new(),
          footer: Vec::new(),
          footnotes: Vec::new(),
          anchors: vec![PlacedAnchor {
            mark: AnchorMark::Heading {
              key: HeadingKey::new(index),
              label: None,
            },
            x: model::Length::ZERO,
            y: model::Length::ZERO,
          }],
          links: Vec::new(),
          index_entries: Vec::new(),
          background_color: None,
        };
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
    assert_eq!(entries[0].link_key, HeadingKey::new(0));
    assert_eq!(entries[1].title_plain, "Sec");
    assert_eq!(entries[1].page_label, "2");
    assert_eq!(entries[1].link_key, HeadingKey::new(1));
  }
}
