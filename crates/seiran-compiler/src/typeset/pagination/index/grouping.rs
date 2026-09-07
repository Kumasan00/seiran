//! 索引の並び順と区分の割り当て
//!
//! ソート（`ja` ロケール固定の ICU `Collator`）と区分見出しへの割り当てが**同じ照合キー・同じ照合順序**
//! から出ることを、この module 1 箇所で保証する。かな正規化表や「ん」の特例は持たない。

use std::cmp::Ordering;

use icu::{
  collator::{
    Collator, CollatorBorrowed,
    options::{CollatorOptions, Strength},
  },
  locale::locale,
};

use crate::typeset::pagination::index::IndexEntry;

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

/// 区分見出しのラベル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexGroupLabel {
  /// [`GROUP_LABELS`] の固定ラベル（A–Z・五十音行）
  Fixed(&'static str),
  /// どのラベル区間にも入らないエントリの受け皿（見出し文字列は style から取る）
  Other,
}

/// 1 区分（見出しラベルと、そこへ入るエントリ列）
#[derive(Debug)]
pub(crate) struct IndexGroup<'a> {
  /// 区分見出しのラベル
  pub label: IndexGroupLabel,
  /// この区分に入るエントリ（照合順のまま）
  pub entries: Vec<&'a IndexEntry>,
}

/// エントリ列を ICU `Collator`（ロケール固定 `ja`）でソートする
///
/// # Panics
///
/// `ja` ロケールの照合データはワークスペースの `icu`（`compiled_data`）に常に同梱されているため、
/// 実運用では発生しない。
pub(crate) fn sort_index_entries(entries: &mut [IndexEntry]) {
  let collator = Collator::try_new(locale!("ja").into(), CollatorOptions::default())
    .expect("ja ロケールの照合データは compiled_data で常に利用可能なはず");
  entries.sort_by(|a, b| return collator.compare(sort_key(a), sort_key(b)));
}

/// エントリの照合キー（`reading` があればそれ、なければ語そのもの）
///
/// ソートと区分の割り当てが同じキー・同じ照合順序から出ることを、この 1 関数で保証する。
fn sort_key(entry: &IndexEntry) -> &str { return entry.reading.as_deref().unwrap_or(&entry.word); }

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
pub(crate) fn assign_index_groups<'a>(entries: &'a [IndexEntry]) -> Vec<IndexGroup<'a>> {
  let collator = primary_collator();
  let mut labeled: Vec<Vec<&'a IndexEntry>> = vec![Vec::new(); GROUP_LABELS.len()];
  let mut other: Vec<&'a IndexEntry> = Vec::new();
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

#[cfg(test)]
mod tests {
  use std::cmp::Ordering;

  use super::{
    GROUP_LABELS, IndexGroupLabel, KANA_RANGE_END, assign_index_groups, primary_collator, sort_index_entries,
  };
  use crate::typeset::pagination::index::{IndexEntry, IndexPageRef};

  fn entry(word: &str, reading: Option<&str>) -> IndexEntry {
    return IndexEntry {
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
      IndexEntry {
        word: "same".to_string(),
        reading: None,
        pages: vec![IndexPageRef {
          label: "1".to_string(),
          link_key: 0,
        }],
      },
      IndexEntry {
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

  /// 語（reading は使わない）だけを並べたエントリ列を作る
  fn entries(words: &[&str]) -> Vec<IndexEntry> { return words.iter().map(|word| return entry(word, None)).collect(); }

  /// 区分割り当ての結果を `(見出しラベル, 語列)` へ畳んで比較しやすくする（other は `"other"`）
  fn group_descs(entries: &[IndexEntry]) -> Vec<(String, Vec<String>)> {
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
}
