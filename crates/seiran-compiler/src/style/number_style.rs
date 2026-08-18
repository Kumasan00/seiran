//! カウンタ番号の数字表記スタイル（アラビア数字・ローマ数字・アルファベット・漢数字）。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 番号の数字表記スタイル
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub(crate) enum NumberStyle {
  /// アラビア数字。例: `1, 2, 3`
  #[default]
  Arabic,
  /// 大文字ローマ数字。例: `I, II, III`
  RomanUpper,
  /// 小文字ローマ数字。例: `i, ii, iii`
  RomanLower,
  /// 大文字アルファベット（Excel 列方式: `A, B, ..., Z, AA, AB, ...`）
  AlphaUpper,
  /// 小文字アルファベット（Excel 列方式: `a, b, ..., z, aa, ab, ...`）
  AlphaLower,
  /// 漢数字（位取り表記: `一, 二, ..., 十, 十一, ..., 二十, ..., 千二百三十四`）
  Kanji,
}

impl NumberStyle {
  /// カウンタ値 `n` を各スタイルでレンダリングする
  ///
  /// `n == 0` のときは空文字列を返す(ローマ数字・アルファベットに 0 が存在しないため)。
  #[must_use]
  pub(crate) fn render(self, n: u32) -> String {
    if n == 0 {
      return String::new();
    }
    return match self {
      Self::Arabic => n.to_string(),
      Self::RomanUpper => render_roman(n, false),
      Self::RomanLower => render_roman(n, true),
      Self::AlphaUpper => render_alpha(n, false),
      Self::AlphaLower => render_alpha(n, true),
      Self::Kanji => render_kanji(n),
    };
  }
}

/// ローマ数字へ変換（標準アルゴリズム、減算記法対応）
fn render_roman(mut n: u32, lower: bool) -> String {
  const TABLE: [(u32, &str); 13] = [
    (1000, "M"),
    (900, "CM"),
    (500, "D"),
    (400, "CD"),
    (100, "C"),
    (90, "XC"),
    (50, "L"),
    (40, "XL"),
    (10, "X"),
    (9, "IX"),
    (5, "V"),
    (4, "IV"),
    (1, "I"),
  ];
  let mut out = String::new();
  for (value, symbol) in TABLE {
    while n >= value {
      out.push_str(symbol);
      n -= value;
    }
  }
  if lower {
    return out.to_ascii_lowercase();
  }
  return out;
}

/// アルファベット列（Excel 方式）へ変換: 1→A, 26→Z, 27→AA, 28→AB, ...
fn render_alpha(mut n: u32, lower: bool) -> String {
  let mut chars: Vec<char> = Vec::new();
  let base_upper = b'A';
  while n > 0 {
    let rem = (n - 1) % 26;
    chars.push((base_upper + rem as u8) as char);
    n = (n - 1) / 26;
  }
  let s: String = chars.into_iter().rev().collect();
  if lower {
    return s.to_ascii_lowercase();
  }
  return s;
}

/// 漢数字の位取り表記に使う 0〜9 の文字
const KANJI_DIGITS: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

/// 漢数字へ変換（位取り、最大 9999 まで対応。超過時は通常表記にフォールバック）
fn render_kanji(n: u32) -> String {
  if n >= 10_000 {
    return n.to_string();
  }
  let mut out = String::new();
  let mut rest = n;

  let thousands = rest / 1000;
  if thousands > 0 {
    if thousands > 1 {
      out.push_str(KANJI_DIGITS[thousands as usize]);
    }
    out.push('千');
    rest %= 1000;
  }
  let hundreds = rest / 100;
  if hundreds > 0 {
    if hundreds > 1 {
      out.push_str(KANJI_DIGITS[hundreds as usize]);
    }
    out.push('百');
    rest %= 100;
  }
  let tens = rest / 10;
  if tens > 0 {
    if tens > 1 {
      out.push_str(KANJI_DIGITS[tens as usize]);
    }
    out.push('十');
    rest %= 10;
  }
  if rest > 0 {
    out.push_str(KANJI_DIGITS[rest as usize]);
  }
  return out;
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn number_style_arabic_renders_decimal() {
    assert_eq!(NumberStyle::Arabic.render(0), "");
    assert_eq!(NumberStyle::Arabic.render(1), "1");
    assert_eq!(NumberStyle::Arabic.render(42), "42");
  }

  #[test]
  fn number_style_roman_upper_renders_standard_form() {
    assert_eq!(NumberStyle::RomanUpper.render(0), "");
    assert_eq!(NumberStyle::RomanUpper.render(1), "I");
    assert_eq!(NumberStyle::RomanUpper.render(4), "IV");
    assert_eq!(NumberStyle::RomanUpper.render(9), "IX");
    assert_eq!(NumberStyle::RomanUpper.render(40), "XL");
    assert_eq!(NumberStyle::RomanUpper.render(1994), "MCMXCIV");
  }

  #[test]
  fn number_style_roman_lower_is_lowercased() {
    assert_eq!(NumberStyle::RomanLower.render(1994), "mcmxciv");
  }

  #[test]
  fn number_style_alpha_upper_uses_excel_overflow() {
    assert_eq!(NumberStyle::AlphaUpper.render(0), "");
    assert_eq!(NumberStyle::AlphaUpper.render(1), "A");
    assert_eq!(NumberStyle::AlphaUpper.render(26), "Z");
    assert_eq!(NumberStyle::AlphaUpper.render(27), "AA");
    assert_eq!(NumberStyle::AlphaUpper.render(52), "AZ");
    assert_eq!(NumberStyle::AlphaUpper.render(53), "BA");
    assert_eq!(NumberStyle::AlphaUpper.render(702), "ZZ");
    assert_eq!(NumberStyle::AlphaUpper.render(703), "AAA");
  }

  #[test]
  fn number_style_alpha_lower_is_lowercased() {
    assert_eq!(NumberStyle::AlphaLower.render(28), "ab");
  }

  #[test]
  fn number_style_kanji_renders_positional() {
    assert_eq!(NumberStyle::Kanji.render(0), "");
    assert_eq!(NumberStyle::Kanji.render(1), "一");
    assert_eq!(NumberStyle::Kanji.render(10), "十");
    assert_eq!(NumberStyle::Kanji.render(11), "十一");
    assert_eq!(NumberStyle::Kanji.render(20), "二十");
    assert_eq!(NumberStyle::Kanji.render(99), "九十九");
    assert_eq!(NumberStyle::Kanji.render(100), "百");
    assert_eq!(NumberStyle::Kanji.render(123), "百二十三");
    assert_eq!(NumberStyle::Kanji.render(1000), "千");
    assert_eq!(NumberStyle::Kanji.render(2345), "二千三百四十五");
    assert_eq!(NumberStyle::Kanji.render(9999), "九千九百九十九");
  }

  #[test]
  fn number_style_kanji_falls_back_above_9999() {
    assert_eq!(NumberStyle::Kanji.render(10_000), "10000");
  }
}
