//! 前付け（タイトルページ・目次）のページ分割オーケストレーション
//!
//! ブロックの組み立て順序自体は `crate::typeset::layout_front_matter` に閉じている。ここでは
//! `BodyPageValues`（seiran 限定の段階型）から目次エントリを組み立てる（phase レベルの計装ごと）
//! だけを担う。

use std::time::Instant;

use tracing::info;

use super::{
  elapsed_ms,
  page_values::BodyPageValues,
  phase_context::{BodyPageFacts, CompileContext},
};
use crate::{
  resolve::HeadingKey,
  typeset::{FrontMatterInput, HeadingRecord, Page, TocEntryInput},
};

/// 前付け（タイトルページ・目次）を生成してページ分割する。
///
/// 前付けは常に 1 段組み（`front_geometry`）で、本文（N 段）とは別に分割する。
pub(super) fn typeset_front_matter(ctx: &CompileContext<'_>, facts: &BodyPageFacts) -> Vec<Page> {
  let stage_start = Instant::now();
  let toc_entries = if ctx.style.toc.enabled {
    collect_toc_entries(&facts.headings, &facts.page_values, &ctx.style.toc)
  } else {
    Vec::new()
  };
  let input = FrontMatterInput {
    config: ctx.config,
    style: ctx.style,
    resources: ctx.resources,
    text_width: ctx.text_width,
    geometry: &ctx.front_geometry,
    breaker: &crate::typeset::KnuthPlassBreaker,
  };
  let pages = crate::typeset::layout_front_matter(&input, &toc_entries);
  info!(
    front_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "前付けのページ分割が完了しました"
  );
  return pages;
}

/// 見出しと本文内ページ index から目次エントリを組み立てる。
///
/// `max_depth` 以上の見出しは除外し、本文の番号スタイルでページラベルを作る。
fn collect_toc_entries(
  headings: &[HeadingRecord],
  page_values: &BodyPageValues,
  toc: &crate::config::TocStyle,
) -> Vec<TocEntryInput> {
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

#[cfg(test)]
mod tests {
  use super::{BodyPageValues, Page, collect_toc_entries};
  use crate::{
    config::{PageNumbering, TocStyle},
    document::HeadingLevel,
    resolve::HeadingKey,
    typeset::{AnchorMark, HeadingRecord, PlacedAnchor},
  };

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
            x: crate::length::Length::ZERO,
            y: crate::length::Length::ZERO,
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
