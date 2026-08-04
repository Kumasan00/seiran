//! 後付け（巻末索引）のページ分割オーケストレーション
//!
//! ブロックの組み立て順序自体は `crate::typeset::layout_back_matter` に閉じている。ここでは本文全
//! ページから索引語を集約する（`BodyPageValues` 依存のため typeset に移せない、phase レベルの
//! 計装ごと）だけを担う。

use std::{
  collections::{BTreeMap, BTreeSet},
  time::Instant,
};

use model::{AnchorMark, Length};
use tracing::info;

use super::{
  elapsed_ms,
  page_values::{BodyPageValues, PageIndex},
  phase_context::{BodyPageFacts, CompileContext},
};
use crate::typeset::{BackMatterInput, IndexEntryInput, IndexPageRef, Page, PlacedAnchor, sort_index_entries};

/// 巻末索引を生成してページ分割する。
///
/// 本文全ページの索引語を集約し、出現ページへ内部リンクの到達先アンカーを事後追加する
/// （`body_pages` の破壊的更新）。`\index` が 1 個もなければ空ページ列を返す。
pub(super) fn typeset_back_matter(
  ctx: &CompileContext<'_>,
  body_pages: &mut [Page],
  facts: &BodyPageFacts,
) -> Vec<Page> {
  let stage_start = Instant::now();
  let entries = collect_index_entries(body_pages, &facts.page_values);
  let input = BackMatterInput {
    style: ctx.style,
    resources: ctx.resources,
    text_width: ctx.text_width,
    geometry: &ctx.back_geometry,
    breaker: &crate::typeset::KnuthPlassBreaker,
  };
  let pages = crate::typeset::layout_back_matter(&input, &entries);
  info!(
    back_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "後付けのページ分割が完了しました"
  );
  return pages;
}

/// 索引語の同一性キー。`PlacedIndexEntry` のページ内重複除去キーと一致させる
/// （同じ語でも `reading` が異なれば別エントリとして扱う）。
type IndexEntryKey = (String, Option<String>);

/// 本文の索引語を集約し、ソート済みの索引エントリを返す。
///
/// 索引語があるページには内部リンク用アンカーも追加する。
pub(super) fn collect_index_entries(
  body_pages: &mut [Page],
  body_page_values: &BodyPageValues,
) -> Vec<IndexEntryInput> {
  let mut occurrences: BTreeMap<IndexEntryKey, BTreeSet<usize>> = BTreeMap::new();
  for (page_index, page) in body_pages.iter().enumerate() {
    for placed in &page.index_entries {
      occurrences.entry((placed.word.clone(), placed.reading.clone())).or_default().insert(page_index);
    }
  }
  if occurrences.is_empty() {
    return Vec::new();
  }

  let anchored_pages: BTreeSet<usize> = occurrences.values().flatten().copied().collect();
  for page_index in anchored_pages {
    body_pages[page_index].anchors.push(PlacedAnchor {
      mark: AnchorMark::IndexPage(page_index),
      x: Length::ZERO,
      y: Length::ZERO,
    });
  }

  let mut entries: Vec<IndexEntryInput> = occurrences
    .into_iter()
    .map(|((word, reading), pages)| {
      return IndexEntryInput {
        word,
        reading,
        pages: pages
          .into_iter()
          .map(|page_index| {
            return IndexPageRef {
              label: body_page_values.body_page_label(PageIndex::new(page_index)),
              link_key: page_index,
            };
          })
          .collect(),
      };
    })
    .collect();
  sort_index_entries(&mut entries);
  return entries;
}

#[cfg(test)]
mod tests {
  use model::AnchorMark;

  use super::{BodyPageValues, collect_index_entries};
  use crate::{
    config::PageNumbering,
    typeset::{Page, PlacedIndexEntry},
  };

  /// 索引語 `index_entries` を持つ 1 ページを作るテストヘルパ
  fn page_with_index_entries(entries: Vec<(&str, Option<&str>)>) -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: Vec::new(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: entries
        .into_iter()
        .map(|(word, reading)| {
          return PlacedIndexEntry {
            word: word.to_string(),
            reading: reading.map(str::to_string),
          };
        })
        .collect(),
      background_color: None,
    };
  }

  #[test]
  fn collect_index_entries_returns_empty_when_no_index_entries() {
    // Arrange — \index が 1 個もない本文ページ
    let mut body_pages = vec![
      page_with_index_entries(vec![]),
      page_with_index_entries(vec![]),
    ];
    let body_page_values = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default());

    // Act
    let entries = collect_index_entries(&mut body_pages, &body_page_values);

    // Assert — 索引エントリを出さず、アンカーも追加しない
    assert!(entries.is_empty());
    assert!(body_pages.iter().all(|p| return p.anchors.is_empty()), "索引が無ければアンカーも追加しない");
  }

  #[test]
  fn collect_index_entries_injects_one_anchor_per_page_with_entries() {
    // Arrange — page0 に 2 語、page1 に重複語（アンカーは 1 個だけになるはず）
    let mut body_pages = vec![
      page_with_index_entries(vec![("犬", None), ("猫", None)]),
      page_with_index_entries(vec![("犬", None)]),
    ];
    let body_page_values = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default());

    // Act
    let entries = collect_index_entries(&mut body_pages, &body_page_values);

    // Assert — page0/page1 それぞれにアンカーが 1 個ずつ追加される
    assert!(!entries.is_empty());
    assert_eq!(body_pages[0].anchors.len(), 1, "page0 は 2 語出現しても事後アンカーは 1 個");
    assert_eq!(body_pages[1].anchors.len(), 1);
    assert!(matches!(body_pages[0].anchors[0].mark, AnchorMark::IndexPage(0)));
    assert!(matches!(body_pages[1].anchors[0].mark, AnchorMark::IndexPage(1)));
  }

  #[test]
  fn collect_index_entries_merges_same_word_and_reading_across_pages() {
    // Arrange — 同じ語(reading なし)が page0/page1 に出現、別語 (word 同じだが reading 違い) は別エントリ
    let mut body_pages = vec![
      page_with_index_entries(vec![("犬", None), ("猫", Some("びょう"))]),
      page_with_index_entries(vec![("犬", None), ("猫", Some("ねこ"))]),
    ];
    let body_page_values = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default());

    // Act
    let entries = collect_index_entries(&mut body_pages, &body_page_values);

    // Assert — 「犬」は 1 エントリに 2 ページ、「猫」は reading 違いで 2 エントリに分かれ各 1 ページ
    let dog = entries.iter().find(|e| return e.word == "犬").expect("犬エントリがあるはず");
    assert_eq!(dog.pages.len(), 2);
    assert_eq!(dog.pages[0].label, "1");
    assert_eq!(dog.pages[1].label, "2");

    let cat_entries: Vec<_> = entries.iter().filter(|e| return e.word == "猫").collect();
    assert_eq!(cat_entries.len(), 2, "reading が異なれば別エントリになるはず");
    assert!(cat_entries.iter().all(|e| return e.pages.len() == 1));
  }
}
