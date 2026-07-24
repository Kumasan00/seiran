//! 後付け（巻末索引）の組み立て・ページ分割・補助型
//!
//! [`super::front_matter`] と対になるモジュールだが、前付けと違い本文の**後**に連結する。索引の
//! ページ番号は本文からの通し（独立した番号体系を持たない）なので、[`super::page_values`] 側は
//! `BodyPageValues::with_back_matter` で本文ページ数へ合算するだけでよい。

use std::collections::{BTreeMap, BTreeSet};

use config::Style;
use font::{FontMetrics, shaper::HarfRustShapers};
use model::{AnchorMark, Block, FontKind, Length, Page, PlacedAnchor, TextAlignment};
use tracing::debug_span;
use typeset::{
  IndexEntryInput, IndexPageRef, IndexSpec, LineBreaker, PageGeometry, TextStyle, build_index_blocks,
  sort_index_entries,
};

use super::{
  compile::{BodyPageFacts, CompileContext},
  page_values::BodyPageValues,
};

/// phase 4: 後付け（巻末索引）を生成してページ分割する。
///
/// 本文全ページの索引語を集約し、出現ページへ内部リンクの到達先アンカーを事後追加する
/// （`body_pages` の破壊的更新）。`\index` が 1 個もなければ空ページ列を返す。
pub(super) fn typeset_back_matter(
  ctx: &CompileContext<'_>,
  body_pages: &mut [Page],
  facts: &BodyPageFacts,
) -> Vec<Page> {
  let back_blocks = assemble_back_matter(body_pages, &facts.page_values, ctx.style, ctx.shapers, ctx.metrics);
  let _span = debug_span!("break_pages", region = "back").entered();
  return break_back_matter(
    back_blocks,
    ctx.text_width,
    &ctx.back_geometry,
    &typeset::KnuthPlassBreaker,
    ctx.style.text.alignment,
  );
}

/// 索引語の同一性キー。`PlacedIndexEntry` のページ内重複除去キーと一致させる
/// （同じ語でも `reading` が異なれば別エントリとして扱う）。
type IndexEntryKey = (String, Option<String>);

/// 全 `body_pages` の索引語を集約し、ソート済みの [`typeset::IndexEntryInput`] 列を返す。
///
/// 併せて、索引語が出現した各ページへ内部リンクの到達先アンカー（[`model::AnchorMark::IndexPage`]）を
/// 事後追加する（`body_pages` を破壊的に更新する副作用を持つ）。フォントに依存しない純粋ロジックの
/// 部分だけを切り出しているので、`build_index_blocks`（フォント依存）を経由せずに単体テストできる。
/// `\index` が 1 個もなければ空ベクタを返す。
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

  // 索引語が出現した各ページへ、内部リンクの到達先アンカーを事後追加する（1 ページにつき 1 個）。
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
              label: body_page_values.body_page_label(page_index),
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

/// 索引ブロック（タイトル → エントリ列）を組み立てる。
///
/// `\index` が 1 個もなければ空ベクタを返す（索引ページを出さない）。
pub(super) fn assemble_back_matter(
  body_pages: &mut [Page],
  body_page_values: &BodyPageValues,
  style: &Style,
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
) -> Vec<Block> {
  let entries = collect_index_entries(body_pages, body_page_values);
  if entries.is_empty() {
    return Vec::new();
  }
  let spec = build_index_spec(style);
  return build_index_blocks(&spec, &entries, shapers, metrics);
}

/// `style.index` / `style.hyperref` から索引生成用の [`typeset::IndexSpec`] を組み立てる。
///
/// ページ番号の文字色は既存の参照リンク色（`style.hyperref.link_color`）に従う（issue #247 の仕様）。
fn build_index_spec(style: &Style) -> IndexSpec {
  let index = &style.index;
  return IndexSpec {
    title: index.title.clone(),
    title_style: TextStyle {
      font_size: index.title_font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
    title_bottom_margin: index.title_bottom_margin,
    entry_style: TextStyle {
      font_size: index.font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
    page_number_style: TextStyle {
      font_size: index.font_size,
      font_kind: FontKind::Serif,
      color: style.hyperref.link_color,
    },
    entry_gap: index.entry_gap,
    line_height_factor: style.text.line_height_factor,
    bottom_margin: index.bottom_margin,
  };
}

/// 索引ブロックを単独でページ分割する。
///
/// 索引専用の段組み（`back_geometry`、`style.index.column_count`）で分割する。`back_blocks` が
/// 空（索引なし）のときは空ページ列を返す。
pub(super) fn break_back_matter(
  back_blocks: Vec<Block>,
  text_width: Length,
  back_geometry: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
) -> Vec<Page> {
  if back_blocks.is_empty() {
    return Vec::new();
  }
  return typeset::break_pages(back_blocks, text_width, back_geometry, breaker, alignment);
}

#[cfg(test)]
mod tests {
  use config::PageNumbering;
  use model::{AnchorMark, Page, PlacedIndexEntry};

  use super::{BodyPageValues, collect_index_entries};

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
