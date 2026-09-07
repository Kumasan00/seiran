//! 巻末索引の生成 — 出現箇所の集約・並び順・区分・ページ番号の表示方針・style 投影・行組み立て
//!
//! 段 4（後付け）から呼ばれる。本文のページ分割が確定した後に走るので、入力は本文ページ列と
//! [`BodyPageFacts`] で足りる。後付けのページ分割は呼び出し元 `back_matter` が持つ。
//! 並び順と区分の割り当ては子 module [`grouping`] に閉じる。

mod grouping;

use std::{
  borrow::Cow,
  collections::{BTreeMap, BTreeSet},
};

use grouping::{IndexGroupLabel, assign_index_groups, sort_index_entries};

use crate::{
  document::FontKind,
  length::Length,
  style::Style,
  typeset::{
    boxes::{AnchorId, AnchorMark, Block, Line, LineLink, LinkTarget, PENALTY_FORBID_BREAK, Page, PlacedAnchor},
    boxing::{LineAccum, Measurer},
    font::FontSystem,
    lowering::TextStyle,
    pagination::{
      context::{BodyPageFacts, TypesetContext},
      page_values::{BodyPageValues, PageIndex},
    },
  },
};

/// 範囲表記の区切り記号（en dash）
///
/// 慣習として固定の定数なので style へは出さない（#508）。
const PAGE_RANGE_SEPARATOR: &str = "–";

/// 範囲表記へ畳む最小の連続ページ数
///
/// 2 ページ連続は「3, 4」のまま残す慣習に合わせた固定値で、style へは出さない（#508）。
const MIN_COLLAPSED_RUN: usize = 3;

/// 索引エントリが指す 1 出現ページ
#[derive(Debug, Clone)]
pub(crate) struct IndexPageRef {
  /// 表示するページ番号ラベル
  pub label: String,
  /// 出現ページの内部リンク到達先（本文内ページ index、0 起点）
  pub link_key: usize,
}

/// 1 索引エントリ
#[derive(Debug, Clone)]
pub(crate) struct IndexEntry {
  /// 索引語（表示テキスト）
  pub word: String,
  /// 読みソートキー（`[reading=...]`）。ソートにのみ使い、表示はしない
  pub reading: Option<String>,
  /// 出現ページ（昇順・重複なし）
  pub pages: Vec<IndexPageRef>,
}

/// 索引語の同一性キー。`PlacedIndexEntry` のページ内重複除去キーと一致させる
/// （同じ語でも `reading` が異なれば別エントリとして扱う）。
type IndexEntryKey = (String, Option<String>);

/// 巻末索引の計測済みブロック列を組み立てる。
///
/// 本文全ページの索引語を集約し、照合順に並べ、区分へ割り当て、ページ番号列を畳んで行に組むまでを
/// この 1 操作に閉じる。`\index` が 1 個もなければ空の `Vec` を返す。
///
/// **副作用**: 索引語が出現する本文ページへ内部リンクの到達先アンカー（`AnchorMark::IndexPage`）を
/// 事後追加する（`body_pages` の破壊的更新）。索引語は座標を持たないため、リンク先は語の位置ではなく
/// 出現ページの先頭になる。
#[must_use]
pub(super) fn build_index_blocks(
  ctx: &TypesetContext<'_>,
  body_pages: &mut [Page],
  facts: &BodyPageFacts,
) -> Vec<Block> {
  let entries = collect_index_entries(body_pages, &facts.page_values);
  if entries.is_empty() {
    return Vec::new();
  }
  let spec = build_index_spec(ctx.style);
  return compose_blocks(&spec, &entries, ctx.resources);
}

/// 本文の索引語を集約し、ソート済みの索引エントリを返す。
///
/// 索引語があるページには内部リンク用アンカーも追加する。
fn collect_index_entries(body_pages: &mut [Page], body_page_values: &BodyPageValues) -> Vec<IndexEntry> {
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

  let mut entries: Vec<IndexEntry> = occurrences
    .into_iter()
    .map(|((word, reading), pages)| {
      return IndexEntry {
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

/// 索引生成に必要なプリミティブ設定。
#[derive(Debug, Clone)]
struct IndexSpec {
  /// 索引ページのタイトル文字列（例: `"Index"`）
  title: String,
  /// タイトル文字列の書体
  title_style: TextStyle,
  /// タイトルとエントリ群の間の縦アキ（pt）
  title_bottom_margin: Length,
  /// エントリの語部分の書体
  entry_style: TextStyle,
  /// ページ番号部分の書体（既存の参照リンク色を反映済み）
  page_number_style: TextStyle,
  /// 語とページ番号列の間の水平アキ（pt）
  entry_gap: Length,
  /// 行高係数。各行の行送り = 書体サイズ × この値
  line_height_factor: f32,
  /// 索引ブロック全体の下余白（pt）
  bottom_margin: Length,
  /// 連続する 3 ページ以上を範囲表記へ畳むか
  collapse_page_ranges: bool,
  /// 区分見出し（五十音行・A–Z）を挟むか
  group_headings: bool,
  /// 区分見出しの書体
  group_style: TextStyle,
  /// 区分見出しの上余白（pt）
  group_top_margin: Length,
  /// 区分見出しと最初のエントリの間の下余白（pt）
  group_bottom_margin: Length,
  /// 受け皿の区分（数字・記号始まり等）の見出し文字列
  group_other_label: String,
}

/// スタイルから索引生成用の [`IndexSpec`] を組み立てる。
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
    collapse_page_ranges: index.collapse_page_ranges,
    group_headings: index.group_headings,
    group_style: TextStyle {
      font_size: index.group_font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
    group_top_margin: index.group_top_margin,
    group_bottom_margin: index.group_bottom_margin,
    group_other_label: index.group_other_label.clone(),
  };
}

/// 索引エントリ列を計測済みのブロック列に変換する
#[must_use]
fn compose_blocks(spec: &IndexSpec, entries: &[IndexEntry], resources: &FontSystem<'_>) -> Vec<Block> {
  if entries.is_empty() {
    return Vec::new();
  }
  let mut measurer = Measurer::new(resources, Length::ZERO, 1.0, None, true);
  let mut blocks: Vec<Block> = Vec::new();

  blocks.push(Block::ComposedLine {
    line: compose_left_line(&mut measurer, &spec.title, spec.title_style),
    leading: spec.title_style.font_size * spec.line_height_factor,
  });
  if spec.title_bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.title_bottom_margin));
  }

  if spec.group_headings {
    for group in assign_index_groups(entries) {
      push_group_heading(&mut blocks, &mut measurer, spec, group.label);
      for entry in group.entries {
        push_entry_line(&mut blocks, &mut measurer, spec, entry);
      }
    }
  } else {
    for entry in entries {
      push_entry_line(&mut blocks, &mut measurer, spec, entry);
    }
  }

  if spec.bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.bottom_margin));
  }
  return blocks;
}

/// 1 エントリ行のブロックを積む
fn push_entry_line(blocks: &mut Vec<Block>, measurer: &mut Measurer<'_>, spec: &IndexSpec, entry: &IndexEntry) {
  blocks.push(Block::ComposedLine {
    line: compose_entry_line(measurer, spec, entry),
    leading: spec.entry_style.font_size * spec.line_height_factor,
  });
}

/// 区分見出しのブロック（上余白・見出し行・分割禁止・下余白）を積む
///
/// 見出し行の直後に [`PENALTY_FORBID_BREAK`] を置くことで、`break_pages` の keep-with-next 機構
/// （`keep_group_end`）が見出し行と直後の 1 エントリを 1 グループとして扱い、見出しが段末・ページ末に
/// 孤立しなくなる。間に挟まる下余白（`Block::Glue`）は内容ブロックではないので走査を妨げない。
fn push_group_heading(blocks: &mut Vec<Block>, measurer: &mut Measurer<'_>, spec: &IndexSpec, label: IndexGroupLabel) {
  if spec.group_top_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.group_top_margin));
  }
  let text = match label {
    IndexGroupLabel::Fixed(label) => label,
    IndexGroupLabel::Other => spec.group_other_label.as_str(),
  };
  blocks.push(Block::ComposedLine {
    line: compose_left_line(measurer, text, spec.group_style),
    leading: spec.group_style.font_size * spec.line_height_factor,
  });
  blocks.push(Block::Penalty {
    value: PENALTY_FORBID_BREAK,
  });
  if spec.group_bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.group_bottom_margin));
  }
}

/// テキストを左端（x=0）からシェーピングして単一行に組む（タイトル行用）
fn compose_left_line(measurer: &mut Measurer<'_>, text: &str, style: TextStyle) -> Line {
  let mut acc = LineAccum::default();
  acc.place(measurer.shape_text(text, style), Length::ZERO);
  return acc.into_line(Vec::new());
}

/// ページ番号列の 1 表示単位
#[derive(Debug)]
enum IndexPageItem<'a> {
  /// 単独ページ
  Single(&'a IndexPageRef),
  /// 畳んだ連続ページ範囲（表示は `first`–`last`、リンク先は `first`）
  Range {
    /// 走りの先頭ページ
    first: &'a IndexPageRef,
    /// 走りの末尾ページ
    last: &'a IndexPageRef,
  },
}

/// ページ参照列を表示単位（単独ページ / 畳んだ連続範囲）へ分ける
///
/// `collapse` が `false` なら全ページが [`IndexPageItem::Single`] になり、従来の表記と一致する。
/// `true` のときは連続が [`MIN_COLLAPSED_RUN`] ページ以上の走りだけを範囲へ畳み、2 ページ連続は
/// 単独ページ 2 つのまま残す。
///
/// `pages` は昇順・重複なしであることを前提にする（本 module の `collect_index_entries` が
/// `BTreeSet<usize>` で保証する）。表示ラベルは `link_key + 1` を
/// ページ番号スタイルで整形したものなので、`link_key` が連続することとページ番号が連続することは
/// 同値であり、ローマ数字などの非算用数字ラベルでもラベル文字列を解析せずに判定できる。
fn group_page_items(pages: &[IndexPageRef], collapse: bool) -> Vec<IndexPageItem<'_>> {
  debug_assert!(
    pages.windows(2).all(|pair| return pair[0].link_key < pair[1].link_key),
    "索引エントリのページは昇順・重複なしで渡されるはず"
  );
  let mut items = Vec::new();
  let mut start = 0;
  while let Some(first) = pages.get(start) {
    let mut end = start;
    if collapse {
      while pages.get(end + 1).is_some_and(|next| return next.link_key == pages[end].link_key + 1) {
        end += 1;
      }
    }
    if end - start + 1 >= MIN_COLLAPSED_RUN {
      items.push(IndexPageItem::Range {
        first,
        last: &pages[end],
      });
    } else {
      items.extend(pages[start..=end].iter().map(IndexPageItem::Single));
    }
    start = end + 1;
  }
  return items;
}

/// 1 エントリを「語 … ページ番号列（カンマ区切り）」の単一行に組む
fn compose_entry_line(measurer: &mut Measurer<'_>, spec: &IndexSpec, entry: &IndexEntry) -> Line {
  let mut acc = LineAccum::default();
  let mut links = Vec::new();

  let mut x = acc.place(measurer.shape_text(&entry.word, spec.entry_style), Length::ZERO);
  if !entry.pages.is_empty() {
    x += spec.entry_gap;
  }

  for (i, item) in group_page_items(&entry.pages, spec.collapse_page_ranges).into_iter().enumerate() {
    if i > 0 {
      x = acc.place(measurer.shape_text(", ", spec.entry_style), x);
    }
    let (text, link_key) = match item {
      IndexPageItem::Single(page) => (Cow::Borrowed(page.label.as_str()), page.link_key),
      IndexPageItem::Range { first, last } => {
        (Cow::Owned(format!("{}{PAGE_RANGE_SEPARATOR}{}", first.label, last.label)), first.link_key)
      },
    };
    let start_x = x;
    x = acc.place(measurer.shape_text(&text, spec.page_number_style), x);
    links.push(LineLink {
      target: LinkTarget::Internal(AnchorId::IndexPage(link_key)),
      x0: start_x,
      x1: x,
    });
  }

  return acc.into_line(links);
}

#[cfg(test)]
mod tests {
  use super::{IndexPageItem, IndexPageRef, build_index_spec, collect_index_entries, group_page_items};
  use crate::{
    length::Length,
    style::{PageNumbering, Style},
    typeset::{
      boxes::{AnchorId, AnchorMark, LinkTarget, Page, PlacedIndexEntry},
      pagination::page_values::BodyPageValues,
    },
  };

  /// 本文内ページ index 列から `IndexPageRef` 列を作る（ラベルは算用数字＝ `index + 1`）
  fn page_refs(link_keys: &[usize]) -> Vec<IndexPageRef> {
    return link_keys
      .iter()
      .map(|&link_key| {
        return IndexPageRef {
          label: (link_key + 1).to_string(),
          link_key,
        };
      })
      .collect();
  }

  /// 表示単位列を `"1"` / `"1-3"`（範囲は先頭-末尾のラベル）の列へ畳んで比較しやすくする
  fn item_descs(items: &[IndexPageItem<'_>]) -> Vec<String> {
    return items
      .iter()
      .map(|item| {
        return match item {
          IndexPageItem::Single(page) => page.label.clone(),
          IndexPageItem::Range { first, last } => format!("{}-{}", first.label, last.label),
        };
      })
      .collect();
  }

  #[test]
  fn group_page_items_keeps_every_page_single_when_disabled() {
    let pages = page_refs(&[0, 1, 2, 3]);

    let items = group_page_items(&pages, false);

    assert_eq!(item_descs(&items), vec!["1", "2", "3", "4"]);
  }

  #[test]
  fn group_page_items_keeps_two_page_run_uncollapsed() {
    let pages = page_refs(&[0, 1]);

    let items = group_page_items(&pages, true);

    assert_eq!(item_descs(&items), vec!["1", "2"], "2 ページ連続は範囲へ畳まない");
  }

  #[test]
  fn group_page_items_collapses_exactly_three_pages() {
    let pages = page_refs(&[0, 1, 2]);

    let items = group_page_items(&pages, true);

    assert_eq!(item_descs(&items), vec!["1-3"]);
  }

  #[test]
  fn group_page_items_mixes_runs_and_single_pages() {
    // Arrange — 3 連続 → 単独 → 2 連続（末尾の 2 連続は畳まない）
    let pages = page_refs(&[0, 1, 2, 4, 6, 7]);

    // Act
    let items = group_page_items(&pages, true);

    // Assert
    assert_eq!(item_descs(&items), vec!["1-3", "5", "7", "8"]);
  }

  #[test]
  fn group_page_items_collapses_whole_range() {
    let pages = page_refs(&[0, 1, 2, 3, 4]);

    let items = group_page_items(&pages, true);

    assert_eq!(item_descs(&items), vec!["1-5"]);
  }

  #[test]
  fn group_page_items_handles_single_page_entry() {
    let pages = page_refs(&[2]);

    let items = group_page_items(&pages, true);

    assert_eq!(item_descs(&items), vec!["3"]);
  }

  #[test]
  fn group_page_items_links_range_to_its_first_page() {
    let pages = page_refs(&[3, 4, 5]);

    let items = group_page_items(&pages, true);

    let IndexPageItem::Range { first, last } = &items[0] else {
      panic!("3 連続は範囲へ畳まれるはず");
    };
    assert_eq!(first.link_key, 3, "リンク先は範囲先頭ページ");
    assert_eq!(last.link_key, 5);
  }

  #[test]
  fn build_index_spec_carries_group_fields_from_style() {
    // Arrange — 既定でない値を style へ入れる（style.toml の差し替えだけで反映されること）
    let mut style = Style::default();
    style.index.group_headings = true;
    style.index.group_font_size = Length::pt(14.0);
    style.index.group_top_margin = Length::pt(9.0);
    style.index.group_bottom_margin = Length::pt(3.0);
    style.index.group_other_label = "その他".to_string();

    // Act
    let spec = build_index_spec(&style);

    // Assert
    assert!(spec.group_headings);
    assert_eq!(spec.group_style.font_size, Length::pt(14.0));
    assert_eq!(spec.group_top_margin, Length::pt(9.0));
    assert_eq!(spec.group_bottom_margin, Length::pt(3.0));
    assert_eq!(spec.group_other_label, "その他");
  }

  #[test]
  fn link_target_wraps_link_key() {
    let key = 3;
    assert!(
      matches!(LinkTarget::Internal(AnchorId::IndexPage(key)), LinkTarget::Internal(AnchorId::IndexPage(k)) if k == key)
    );
  }

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
      content_origin_x: Length::ZERO,
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
