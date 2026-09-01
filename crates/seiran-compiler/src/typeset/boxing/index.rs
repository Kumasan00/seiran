//! 巻末索引ブロックの生成パス

use std::borrow::Cow;

use icu::{
  collator::{Collator, options::CollatorOptions},
  locale::locale,
};

use crate::{
  document::FontKind,
  length::Length,
  style::Style,
  typeset::{
    boxes::{AnchorId, Block, HBox, Line, LineLink, LinkTarget, PositionedBox},
    boxing::Measurer,
    font::FontSystem,
    lowering::TextStyle,
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

/// 索引生成に必要なプリミティブ設定。
#[derive(Debug, Clone)]
pub(crate) struct IndexSpec {
  /// 索引ページのタイトル文字列（例: `"Index"`）
  pub title: String,
  /// タイトル文字列の書体
  pub title_style: TextStyle,
  /// タイトルとエントリ群の間の縦アキ（pt）
  pub title_bottom_margin: Length,
  /// エントリの語部分の書体
  pub entry_style: TextStyle,
  /// ページ番号部分の書体（既存の参照リンク色を反映済み）
  pub page_number_style: TextStyle,
  /// 語とページ番号列の間の水平アキ（pt）
  pub entry_gap: Length,
  /// 行高係数。各行の行送り = 書体サイズ × この値
  pub line_height_factor: f32,
  /// 索引ブロック全体の下余白（pt）
  pub bottom_margin: Length,
  /// 連続する 3 ページ以上を範囲表記へ畳むか
  pub collapse_page_ranges: bool,
}

/// 索引エントリが指す 1 出現ページ
#[derive(Debug, Clone)]
pub(crate) struct IndexPageRef {
  /// 表示するページ番号ラベル
  pub label: String,
  /// 出現ページの内部リンク到達先（本文内ページ index、0 起点）
  pub link_key: usize,
}

/// 1 索引エントリの入力
#[derive(Debug, Clone)]
pub(crate) struct IndexEntryInput {
  /// 索引語（表示テキスト）
  pub word: String,
  /// 読みソートキー（`[reading=...]`）。ソートにのみ使い、表示はしない
  pub reading: Option<String>,
  /// 出現ページ（昇順・重複なし）
  pub pages: Vec<IndexPageRef>,
}

/// エントリ列を ICU `Collator`（ロケール固定 `ja`）でソートする
///
/// # Panics
///
/// `ja` ロケールの照合データはワークスペースの `icu`（`compiled_data`）に常に同梱されているため、
/// 実運用では発生しない。
pub(crate) fn sort_index_entries(entries: &mut [IndexEntryInput]) {
  let collator = Collator::try_new(locale!("ja").into(), CollatorOptions::default())
    .expect("ja ロケールの照合データは compiled_data で常に利用可能なはず");
  entries.sort_by(|a, b| {
    let key_a = a.reading.as_deref().unwrap_or(&a.word);
    let key_b = b.reading.as_deref().unwrap_or(&b.word);
    return collator.compare(key_a, key_b);
  });
}

/// スタイルから索引生成用の [`IndexSpec`] を組み立てる。
pub(crate) fn build_index_spec(style: &Style) -> IndexSpec {
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
  };
}

/// 索引エントリ列を計測済みのブロック列に変換する
#[must_use]
pub(crate) fn build_index_blocks(
  spec: &IndexSpec,
  entries: &[IndexEntryInput],
  resources: &FontSystem<'_>,
) -> Vec<Block> {
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

  let entry_leading = spec.entry_style.font_size * spec.line_height_factor;
  for entry in entries {
    blocks.push(Block::ComposedLine {
      line: compose_entry_line(&mut measurer, spec, entry),
      leading: entry_leading,
    });
  }

  if spec.bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.bottom_margin));
  }
  return blocks;
}

/// 単一行を組み立てる際の累積状態（配置済みボックス・行の高さ）
#[derive(Default)]
struct LineAccum {
  /// 配置済みボックス列
  boxes: Vec<PositionedBox>,
  /// 行の高さ（ベースラインより上）
  height: Length,
  /// 行の深さ（ベースラインより下）
  depth: Length,
}

impl LineAccum {
  /// `HBox` 列を `x_start` から水平に並べて追加し、行の高さ・深さを更新する。末尾の x を返す
  fn place(&mut self, hboxes: Vec<HBox>, x_start: Length) -> Length {
    let mut x = x_start;
    for hbox in hboxes {
      self.height = self.height.max(hbox.height);
      self.depth = self.depth.max(hbox.depth);
      self.boxes.push(PositionedBox {
        content: hbox.content,
        x,
        dy: Length::ZERO,
        width: hbox.width,
      });
      x += hbox.width;
    }
    return x;
  }

  /// 累積した内容を `Line`（段落最終行扱い）に確定する
  fn into_line(self, links: Vec<LineLink>) -> Line {
    return Line {
      boxes: self.boxes,
      height: self.height,
      depth: self.depth,
      is_last: true,
      links,
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
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
/// `pages` は昇順・重複なしであることを前提にする（`typeset::pagination::back_matter` の
/// `collect_index_entries` が `BTreeSet<usize>` で保証する）。表示ラベルは `link_key + 1` を
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
fn compose_entry_line(measurer: &mut Measurer<'_>, spec: &IndexSpec, entry: &IndexEntryInput) -> Line {
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
  use super::{IndexEntryInput, IndexPageItem, IndexPageRef, group_page_items, sort_index_entries};
  use crate::typeset::boxes::{AnchorId, LinkTarget};

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

  fn entry(word: &str, reading: Option<&str>) -> IndexEntryInput {
    return IndexEntryInput {
      word: word.to_string(),
      reading: reading.map(str::to_string),
      pages: vec![IndexPageRef {
        label: "1".to_string(),
        link_key: 0,
      }],
    };
  }

  #[test]
  fn sort_index_entries_prefers_reading_over_word() {
    // Arrange
    let mut entries = vec![entry("後", Some("うしろ")), entry("前", Some("あいうえお"))];

    // Act
    sort_index_entries(&mut entries);

    // Assert
    assert_eq!(entries[0].word, "前");
    assert_eq!(entries[1].word, "後");
  }

  #[test]
  fn sort_index_entries_falls_back_to_word_without_reading() {
    // Arrange
    let mut entries = vec![entry("b", None), entry("a", None)];

    // Act
    sort_index_entries(&mut entries);

    // Assert
    assert_eq!(entries[0].word, "a");
    assert_eq!(entries[1].word, "b");
  }

  #[test]
  fn sort_index_entries_is_stable_for_equal_keys() {
    // Arrange
    let mut entries = vec![
      IndexEntryInput {
        word: "same".to_string(),
        reading: None,
        pages: vec![IndexPageRef {
          label: "1".to_string(),
          link_key: 0,
        }],
      },
      IndexEntryInput {
        word: "same".to_string(),
        reading: None,
        pages: vec![IndexPageRef {
          label: "2".to_string(),
          link_key: 1,
        }],
      },
    ];

    // Act
    sort_index_entries(&mut entries);

    // Assert
    assert_eq!(entries[0].pages[0].label, "1");
    assert_eq!(entries[1].pages[0].label, "2");
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
  fn link_target_wraps_link_key() {
    let key = 3;
    assert!(
      matches!(LinkTarget::Internal(AnchorId::IndexPage(key)), LinkTarget::Internal(AnchorId::IndexPage(k)) if k == key)
    );
  }
}
