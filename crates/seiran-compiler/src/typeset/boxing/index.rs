//! 巻末索引ブロックの生成パス

use std::{borrow::Cow, cmp::Ordering};

use icu::{
  collator::{
    Collator, CollatorBorrowed,
    options::{CollatorOptions, Strength},
  },
  locale::locale,
};

use crate::{
  document::FontKind,
  length::Length,
  style::Style,
  typeset::{
    boxes::{AnchorId, Block, HBox, Line, LineLink, LinkTarget, PENALTY_FORBID_BREAK, PositionedBox},
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

/// 区分見出しのラベル固定表（A–Z 26 個 + 五十音行 10 個）
///
/// CLDR の `ja` index characters と同じ並びで、配列順がそのまま区分の出力順（A–Z → 五十音行）になる。
/// 言語慣習の固定表なので style へは出さない（#509）。受け皿の見出しだけは
/// `IndexSpec::group_other_label` で差し替えられる。
const GROUP_LABELS: [&str; 36] = [
  "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
  "X", "Y", "Z", "あ", "か", "さ", "た", "な", "は", "ま", "や", "ら", "わ",
];

/// 五十音の最後の文字。これより後に照合される先頭文字を持つキーは受け皿（other）へ入る
///
/// ICU の `AlphabeticIndex` が script 境界で決める overflow の判定を、ラベル固定表と同じ
/// 慣習定数で代替する（#509）。`ja` 照合は Latin → かな → 漢字 → その他の順に並べ替えるため、
/// reading の無い漢字語やギリシャ文字始まりの語はこの判定で受け皿へ落ちる。
const KANA_RANGE_END: &str = "ん";

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
  /// 区分見出し（五十音行・A–Z）を挟むか
  pub group_headings: bool,
  /// 区分見出しの書体
  pub group_style: TextStyle,
  /// 区分見出しの上余白（pt）
  pub group_top_margin: Length,
  /// 区分見出しと最初のエントリの間の下余白（pt）
  pub group_bottom_margin: Length,
  /// 受け皿の区分（数字・記号始まり等）の見出し文字列
  pub group_other_label: String,
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
  entries.sort_by(|a, b| return collator.compare(sort_key(a), sort_key(b)));
}

/// エントリの照合キー（`reading` があればそれ、なければ語そのもの）
///
/// ソートと区分の割り当てが同じキー・同じ照合順序から出ることを、この 1 関数で保証する。
fn sort_key(entry: &IndexEntryInput) -> &str { return entry.reading.as_deref().unwrap_or(&entry.word); }

/// 区分見出しのラベル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexGroupLabel {
  /// [`GROUP_LABELS`] の固定ラベル（A–Z・五十音行）
  Fixed(&'static str),
  /// どのラベル区間にも入らないエントリの受け皿（見出し文字列は style から取る）
  Other,
}

/// 1 区分（見出しラベルと、そこへ入るエントリ列）
#[derive(Debug)]
struct IndexGroup<'a> {
  /// 区分見出しのラベル
  label: IndexGroupLabel,
  /// この区分に入るエントリ（照合順のまま）
  entries: Vec<&'a IndexEntryInput>,
}

/// 一次強度（大文字 / 小文字・濁点 / 半濁点・カナ種・小書きを同一視する照合レベル）の照合器を作る
///
/// # Panics
///
/// [`sort_index_entries`] と同じ理由で、`ja` の照合データは `compiled_data` に常に同梱されている。
fn primary_collator() -> CollatorBorrowed<'static> {
  let mut options = CollatorOptions::default();
  options.strength = Some(Strength::Primary);
  return Collator::try_new(locale!("ja").into(), options)
    .expect("ja ロケールの照合データは compiled_data で常に利用可能なはず");
}

/// 照合キーが属する [`GROUP_LABELS`] の添字を返す。受け皿へ入るなら `None`
///
/// ICU `AlphabeticIndex` と同じ区間割り当て — キーがラベル L 以上・次ラベル未満なら L の区分。
/// 比較は一次強度なので「がぎ」は「か」と等しい重みから始まり か 行、「ピアノ」は は 行、
/// 「ん」は わ より後に照合されるので わ 行になる（個別の正規化規則は持たない）。
///
/// 受け皿は 2 方向から来る。先頭ラベル `"A"` より前に照合されるもの（数字・記号）は underflow、
/// 最終ラベルの区間を超えるものは overflow。overflow の判定だけキー全体ではなく**先頭文字**を
/// [`KANA_RANGE_END`] と比べる — キー全体だと接頭辞規則で `"んご" > "ん"` となり、「ん」始まりが
/// 受け皿へ落ちてしまうため。
fn group_index_of(collator: &CollatorBorrowed<'_>, key: &str) -> Option<usize> {
  let index = GROUP_LABELS.iter().rposition(|label| return collator.compare(label, key) != Ordering::Greater)?;
  let mut buffer = [0u8; 4];
  let first = key.chars().next()?.encode_utf8(&mut buffer);
  if collator.compare(first, KANA_RANGE_END) == Ordering::Greater {
    return None;
  }
  return Some(index);
}

/// 照合順に並んだエントリ列を区分へ割り当てる（空の区分は返さない）
///
/// 返る順序は A–Z → 五十音行 → 受け皿（末尾）。区分内はエントリ列の順（＝照合順）のまま並べ替えない。
/// 受け皿は underflow と overflow を 1 つに統合したもので、入力が照合順である限り
/// 「underflow < ラベル区間 < overflow」なので連結しただけで照合順を保つ。
fn assign_index_groups<'a>(entries: &'a [IndexEntryInput]) -> Vec<IndexGroup<'a>> {
  let collator = primary_collator();
  let mut labeled: Vec<Vec<&'a IndexEntryInput>> = vec![Vec::new(); GROUP_LABELS.len()];
  let mut other: Vec<&'a IndexEntryInput> = Vec::new();
  for entry in entries {
    match group_index_of(&collator, sort_key(entry)) {
      Some(index) => labeled[index].push(entry),
      None => other.push(entry),
    }
  }

  let mut groups: Vec<IndexGroup<'a>> = labeled
    .into_iter()
    .zip(GROUP_LABELS)
    .filter(|(entries, _)| return !entries.is_empty())
    .map(|(entries, label)| {
      return IndexGroup {
        label: IndexGroupLabel::Fixed(label),
        entries,
      };
    })
    .collect();
  if !other.is_empty() {
    groups.push(IndexGroup {
      label: IndexGroupLabel::Other,
      entries: other,
    });
  }
  return groups;
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
fn push_entry_line(blocks: &mut Vec<Block>, measurer: &mut Measurer<'_>, spec: &IndexSpec, entry: &IndexEntryInput) {
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
  use std::cmp::Ordering;

  use super::{
    GROUP_LABELS, IndexEntryInput, IndexGroupLabel, IndexPageItem, IndexPageRef, KANA_RANGE_END, assign_index_groups,
    build_index_spec, group_page_items, primary_collator, sort_index_entries,
  };
  use crate::{
    length::Length,
    style::Style,
    typeset::boxes::{AnchorId, LinkTarget},
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

  /// 語（reading は使わない）だけを並べたエントリ列を作る
  fn entries(words: &[&str]) -> Vec<IndexEntryInput> {
    return words.iter().map(|word| return entry(word, None)).collect();
  }

  /// 区分割り当ての結果を `(見出しラベル, 語列)` へ畳んで比較しやすくする（other は `"other"`）
  fn group_descs(entries: &[IndexEntryInput]) -> Vec<(String, Vec<String>)> {
    return assign_index_groups(entries)
      .into_iter()
      .map(|group| {
        let label = match group.label {
          IndexGroupLabel::Fixed(label) => label.to_string(),
          IndexGroupLabel::Other => "other".to_string(),
        };
        return (label, group.entries.iter().map(|entry| return entry.word.clone()).collect());
      })
      .collect();
  }

  #[test]
  fn primary_collation_folds_voicing_case_and_kana_kind() {
    // 区分割り当てが依存する一次強度の事実（濁点・半濁点・小書き・カナ種・大文字小文字の同一視）
    let collator = primary_collator();

    assert_eq!(collator.compare("が", "か"), Ordering::Equal, "濁点は一次強度で無視される");
    assert_eq!(collator.compare("ピ", "ひ"), Ordering::Equal, "半濁点とカナ種は一次強度で無視される");
    assert_eq!(collator.compare("ぁ", "あ"), Ordering::Equal, "小書きは一次強度で無視される");
    assert_eq!(collator.compare("a", "A"), Ordering::Equal, "大文字小文字は一次強度で無視される");
  }

  #[test]
  fn primary_collation_orders_labels_and_boundaries() {
    // 固定表（ラベル並び・受け皿の境界）が前提にしている照合順
    let collator = primary_collator();

    assert!(GROUP_LABELS.windows(2).all(|pair| return collator.compare(pair[0], pair[1]) == Ordering::Less));
    assert_eq!(collator.compare("3", "A"), Ordering::Less, "数字は先頭ラベルより前（受け皿）");
    assert_eq!(collator.compare("!", "A"), Ordering::Less, "記号は先頭ラベルより前（受け皿）");
    assert_eq!(collator.compare("ん", KANA_RANGE_END), Ordering::Equal, "「ん」は五十音の末尾そのもの");
    assert_eq!(collator.compare("漢", KANA_RANGE_END), Ordering::Greater, "漢字はかなより後（受け皿）");
  }

  #[test]
  fn assign_index_groups_places_kana_by_collation_interval() {
    // Arrange — 濁音・半濁音・カタカナ・「ん」始まり
    let input = entries(&["がぎ", "ピアノ", "んご", "はな"]);

    // Act
    let groups = group_descs(&input);

    // Assert — 個別の正規化規則ではなく区間割り当てから行が決まる
    assert_eq!(
      groups,
      vec![
        ("か".to_string(), vec!["がぎ".to_string()]),
        ("は".to_string(), vec!["ピアノ".to_string(), "はな".to_string()]),
        ("わ".to_string(), vec!["んご".to_string()]),
      ]
    );
  }

  #[test]
  fn assign_index_groups_folds_latin_case_into_one_group() {
    // Arrange — 小文字始まりが受け皿へ落ちないこと（一次強度で比較する理由）
    let input = entries(&["Apricot", "apple", "Banana"]);

    // Act
    let groups = group_descs(&input);

    // Assert
    assert_eq!(
      groups,
      vec![
        ("A".to_string(), vec!["Apricot".to_string(), "apple".to_string()]),
        ("B".to_string(), vec!["Banana".to_string()]),
      ]
    );
  }

  #[test]
  fn assign_index_groups_merges_underflow_and_overflow_into_other() {
    // Arrange — 数字・記号（underflow）と reading の無い漢字語（overflow）。照合順に並んだ入力を渡す
    let input = entries(&["!important", "3月", "あさひ", "漢字"]);

    // Act
    let groups = group_descs(&input);

    // Assert — 受け皿は 1 つだけで末尾、内部は照合順（underflow → overflow）のまま
    assert_eq!(
      groups,
      vec![
        ("あ".to_string(), vec!["あさひ".to_string()]),
        (
          "other".to_string(),
          vec![
            "!important".to_string(),
            "3月".to_string(),
            "漢字".to_string()
          ]
        ),
      ]
    );
  }

  #[test]
  fn assign_index_groups_orders_groups_latin_then_kana_then_other() {
    // Arrange
    let input = entries(&["apple", "Zebra", "あさひ", "わたし", "漢字"]);

    // Act
    let labels: Vec<String> = group_descs(&input).into_iter().map(|(label, _)| return label).collect();

    // Assert — A–Z → 五十音行 → 受け皿（末尾）。エントリのない区分は出てこない
    assert_eq!(labels, vec!["A", "Z", "あ", "わ", "other"]);
  }

  #[test]
  fn assign_index_groups_uses_reading_as_the_group_key() {
    // Arrange — 表示語は漢字でも reading があればその行へ入る（ソートと同じキー）
    let input = vec![entry("朝日", Some("あさひ")), entry("季節", Some("きせつ"))];

    // Act
    let groups = group_descs(&input);

    // Assert
    assert_eq!(
      groups,
      vec![
        ("あ".to_string(), vec!["朝日".to_string()]),
        ("か".to_string(), vec!["季節".to_string()]),
      ]
    );
  }

  #[test]
  fn assign_index_groups_returns_nothing_for_no_entries() {
    assert!(assign_index_groups(&[]).is_empty());
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
}
