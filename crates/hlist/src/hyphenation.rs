//! 欧文ハイフネーション — 語中の分割可能位置の探索（純粋関数）
//!
//! `hypher`（Liang のアルゴリズム・言語別パターン）で欧文単語を音節に分け、
//! 語中の分割可能バイト位置を求める。フォント・シェーピングに依存しない純粋処理のため、
//! [`crate::break_opportunities`] から呼ばれ単体テストできる。
//! パターンはコンパイル時に埋め込まれる（`icu` の `compiled_data` と同じ思想）。
//! 実際にハイフンのグリフを計測・挿入するのは `layout`（フォントを持つ唯一のクレート）で、
//! ここは分割位置（バイトオフセット）だけを返す。

use std::ops::Range;

pub use hypher::Lang;
use hypher::hyphenate;

/// BCP 47 言語タグをハイフネーション言語へ解決する
///
/// 主言語サブタグ（`en-US` の `en`）を ISO 639-1 コードとして [`Lang::from_iso`] に渡す。
/// `None`・2 文字でないコード・パターンを持たない言語（Cargo feature 未有効を含む）は
/// すべて `None` を返し、呼び出し側でハイフネーションなし（現状どおり）に落とす。
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
///
/// テキストを英字の連続（語）に区切り、各語を `hypher` で音節分割して境界位置を集める。
/// 各オフセットは「その位置の直前で折り返せる」ことを表し、語の先頭・末尾は含まない
/// （`hypher` が言語ごとの左右最小字数を既定で適用するため、語端付近には落ちない）。
/// 大文字始まりの語も対象（`hypher` は内部で小文字化してパターン照合し、返す音節は原文の切片）。
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
///
/// 英字以外（空白・約物・数字・ハイフン等）は語の区切りになる。既存ハイフンを含む複合語は
/// ハイフンで別々の語に分かれ、各部分が独立にハイフネーションされる（ハイフン直後での分割は
/// ICU の分割可能点＝`Penalty` が別途担う）。
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
    // "en" / "en-US" は English、大文字も許容
    assert_eq!(resolve(Some("en")), Some(Lang::English));
    assert_eq!(resolve(Some("en-US")), Some(Lang::English));
    assert_eq!(resolve(Some("DE")), Some(Lang::German));
    assert_eq!(resolve(Some("fr-FR")), Some(Lang::French));
  }

  #[test]
  fn resolve_returns_none_for_unsupported_or_missing() {
    // 未指定・パターン非搭載言語（ja）・3 文字コードはすべて None
    assert_eq!(resolve(None), None);
    assert_eq!(resolve(Some("ja")), None);
    assert_eq!(resolve(Some("und")), None);
  }

  #[test]
  fn hyphenation_points_for_known_english_word() {
    // "hyphenation" は hy-phen-ation（English）に分かれ、語中に 2 つの分割位置を持つ
    let points = hyphenation_points("hyphenation", Lang::English);

    // 各位置は語頭・語末を含まず、原文のバイト境界に一致する
    assert_eq!(points, vec![2, 6], "{points:?}");
    assert_eq!(&"hyphenation"[..2], "hy");
    assert_eq!(&"hyphenation"[..6], "hyphen");
  }

  #[test]
  fn hyphenation_points_offsets_are_absolute_in_text() {
    // 語がテキスト途中にあってもオフセットは絶対位置になる
    let points = hyphenation_points("a hyphenation", Lang::English);

    // "a " (2 バイト) ぶんずれる
    assert_eq!(points, vec![4, 8], "{points:?}");
  }

  #[test]
  fn short_word_has_no_hyphenation_points() {
    // 左右最小字数に満たない短い語は分割位置を持たない
    assert!(hyphenation_points("the", Lang::English).is_empty());
    assert!(hyphenation_points("hello", Lang::English).is_empty());
  }

  #[test]
  fn word_ranges_splits_on_non_alphabetic() {
    // 英字の連続だけを語として抽出する（ハイフン・空白・数字で区切る）
    assert_eq!(word_ranges("well-known 42"), vec![0..4, 5..10]);
    assert_eq!(word_ranges("plain"), vec![0..5]);
    assert_eq!(word_ranges("  "), Vec::<std::ops::Range<usize>>::new());
  }
}
