//! 欧文ハイフネーション — 語中の分割可能位置の探索（純粋関数）

use std::ops::Range;

pub use hypher::Lang;
use hypher::hyphenate;

/// BCP 47 言語タグをハイフネーション言語へ解決する
#[must_use]
pub fn resolve(language: Option<&str>) -> Option<Lang> {
  let primary = language?.split(['-', '_']).next()?;
  let bytes = primary.as_bytes();
  if bytes.len() != 2 {
    return None;
  }
  let code = [bytes[0].to_ascii_lowercase(), bytes[1].to_ascii_lowercase()];
  return Lang::from_iso(code);
}

/// テキスト内の語中分割可能位置（バイトオフセット）を昇順で返す
#[must_use]
pub(crate) fn hyphenation_points(text: &str, lang: Lang) -> Vec<usize> {
  let mut points = Vec::new();
  for range in word_ranges(text) {
    let word = &text[range.clone()];
    let mut syllables = hyphenate(word, lang);
    // 先頭音節ぶん進めた位置が最初の分割境界。以降の各音節先頭が分割可能位置になる
    let Some(first) = syllables.next() else {
      continue;
    };
    let mut offset = range.start + first.len();
    for syllable in syllables {
      points.push(offset);
      offset += syllable.len();
    }
  }
  return points;
}

/// テキストを英字の連続（語）のバイト範囲に区切る
fn word_ranges(text: &str) -> Vec<Range<usize>> {
  let mut ranges = Vec::new();
  let mut start: Option<usize> = None;
  for (i, ch) in text.char_indices() {
    if ch.is_alphabetic() {
      start.get_or_insert(i);
    } else if let Some(s) = start.take() {
      ranges.push(s..i);
    }
  }
  if let Some(s) = start {
    ranges.push(s..text.len());
  }
  return ranges;
}

#[cfg(test)]
mod tests {
  use super::{Lang, hyphenation_points, resolve, word_ranges};

  #[test]
  fn resolve_maps_primary_subtag_to_lang() {
    assert_eq!(resolve(Some("en")), Some(Lang::English));
    assert_eq!(resolve(Some("en-US")), Some(Lang::English));
    assert_eq!(resolve(Some("DE")), Some(Lang::German));
    assert_eq!(resolve(Some("fr-FR")), Some(Lang::French));
  }

  #[test]
  fn resolve_returns_none_for_unsupported_or_missing() {
    assert_eq!(resolve(None), None);
    assert_eq!(resolve(Some("ja")), None);
    assert_eq!(resolve(Some("und")), None);
  }

  #[test]
  fn hyphenation_points_for_known_english_word() {
    let points = hyphenation_points("hyphenation", Lang::English);

    assert_eq!(points, vec![2, 6], "{points:?}");
    assert_eq!(&"hyphenation"[..2], "hy");
    assert_eq!(&"hyphenation"[..6], "hyphen");
  }

  #[test]
  fn hyphenation_points_offsets_are_absolute_in_text() {
    let points = hyphenation_points("a hyphenation", Lang::English);

    assert_eq!(points, vec![4, 8], "{points:?}");
  }

  #[test]
  fn short_word_has_no_hyphenation_points() {
    assert!(hyphenation_points("the", Lang::English).is_empty());
    assert!(hyphenation_points("hello", Lang::English).is_empty());
  }

  #[test]
  fn word_ranges_splits_on_non_alphabetic() {
    assert_eq!(word_ranges("well-known 42"), vec![0..4, 5..10]);
    assert_eq!(word_ranges("plain"), vec![0..5]);
    assert_eq!(word_ranges("  "), Vec::<std::ops::Range<usize>>::new());
  }
}
