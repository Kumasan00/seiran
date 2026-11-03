//! 数値（`document::Numeral`）を style.toml `[quantity]` の書式の部品列（[`QuantityPiece`]）に整える
//!
//! 部品列は本文・数式の箱（boxing）と PDF しおりの平文（[`plain_text`]）の両方の元になる。

use crate::{
  document::{MathSpace, NumberSign, Numeral},
  style::{DecimalMarker, DigitGroupSeparator, ExponentProduct, QuantityStyle},
  typeset::lowering::QuantityPiece,
};

/// 桁区切りを入れない最大の桁数（4 桁は `group_four_digits` のときだけ区切る）
const UNGROUPED_DIGITS: usize = 3;

/// 数値を `style` の書式の部品列にする（隣り合う字は 1 つの [`QuantityPiece::Text`] に畳む）
///
/// 負号は U+2212。指数があれば `仮数 × 10^指数` の形にし、積の記号の前後は中アキ。
pub(in crate::typeset) fn number_pieces(numeral: &Numeral, style: &QuantityStyle) -> Vec<QuantityPiece> {
  let mut pieces = Vec::new();
  match numeral.sign {
    Some(NumberSign::Plus) => push_text(&mut pieces, "+"),
    Some(NumberSign::Minus) => push_text(&mut pieces, "\u{2212}"),
    None => {},
  }
  push_grouped(&mut pieces, &numeral.integer, true, style);
  if let Some(fraction) = &numeral.fraction {
    push_text(
      &mut pieces,
      match style.decimal_marker {
        DecimalMarker::Period => ".",
        DecimalMarker::Comma => ",",
      },
    );
    push_grouped(&mut pieces, fraction, false, style);
  }
  if let Some(exponent) = &numeral.exponent {
    pieces.push(QuantityPiece::Space(MathSpace::Medium));
    push_text(
      &mut pieces,
      match style.exponent_product {
        ExponentProduct::Times => "\u{00D7}",
        ExponentProduct::Dot => "\u{22C5}",
      },
    );
    pieces.push(QuantityPiece::Space(MathSpace::Medium));
    push_text(&mut pieces, "10");
    pieces.push(QuantityPiece::Superscript(exponent.replace('-', "\u{2212}")));
  }
  return pieces;
}

/// 部品列を PDF しおりの平文にする（細アキ U+202F・中アキ U+205F・上付きは Unicode の上付き字）
pub(super) fn plain_text(pieces: &[QuantityPiece]) -> String {
  let mut out = String::new();
  for piece in pieces {
    match piece {
      QuantityPiece::Text(text) => out.push_str(text),
      QuantityPiece::Space(MathSpace::Thin) => out.push('\u{202F}'),
      QuantityPiece::Space(MathSpace::Medium) => out.push('\u{205F}'),
      QuantityPiece::Space(MathSpace::Thick | MathSpace::Quad | MathSpace::NegativeThin) => {
        unreachable!("number_pieces が作るアキは細アキ（桁区切り）と中アキ（積の記号の前後）だけ")
      },
      QuantityPiece::Superscript(text) => out.extend(text.chars().map(superscript_char)),
    }
  }
  return out;
}

/// 指数の字（ASCII 数字か U+2212）を Unicode の上付き字にする
fn superscript_char(ch: char) -> char {
  return match ch {
    '0' => '\u{2070}',
    '1' => '\u{00B9}',
    '2' => '\u{00B2}',
    '3' => '\u{00B3}',
    '4' => '\u{2074}',
    '5' => '\u{2075}',
    '6' => '\u{2076}',
    '7' => '\u{2077}',
    '8' => '\u{2078}',
    '9' => '\u{2079}',
    '\u{2212}' => '\u{207B}',
    _ => unreachable!("指数は frontend の数値の文法を通った数字列で、負号は number_pieces が U+2212 にしてある"),
  };
}

/// 字 `text` を末尾の字の部品に足す（末尾が字でなければ新しい部品にする）
fn push_text(pieces: &mut Vec<QuantityPiece>, text: &str) {
  if let Some(QuantityPiece::Text(last)) = pieces.last_mut() {
    last.push_str(text);
    return;
  }
  pieces.push(QuantityPiece::Text(text.to_string()));
}

/// 数字列 `digits` を 3 桁ごとに区切って積む（`from_right` なら整数部として右から、偽なら小数部として左から）
fn push_grouped(pieces: &mut Vec<QuantityPiece>, digits: &str, from_right: bool, style: &QuantityStyle) {
  let len = digits.len();
  let grouped = style.digit_group_separator != DigitGroupSeparator::None
    && (len > UNGROUPED_DIGITS + 1 || (len == UNGROUPED_DIGITS + 1 && style.group_four_digits));
  if !grouped {
    push_text(pieces, digits);
    return;
  }
  // 数字列は ASCII なので、バイト位置で切ってよい
  let head = if from_right {
    match len % UNGROUPED_DIGITS {
      0 => UNGROUPED_DIGITS,
      rest => rest,
    }
  } else {
    UNGROUPED_DIGITS
  };
  let mut start = 0;
  let mut end = head;
  while start < len {
    if start > 0 {
      match style.digit_group_separator {
        DigitGroupSeparator::Thin => pieces.push(QuantityPiece::Space(MathSpace::Thin)),
        DigitGroupSeparator::Comma => push_text(pieces, ","),
        DigitGroupSeparator::Period => push_text(pieces, "."),
        DigitGroupSeparator::None => unreachable!("区切りが none なら grouped は偽で、ここへは来ない"),
      }
    }
    push_text(pieces, &digits[start..end]);
    start = end;
    end = (end + UNGROUPED_DIGITS).min(len);
  }
}

#[cfg(test)]
mod tests {
  use super::{number_pieces, plain_text};
  use crate::{
    document::{MathSpace, NumberSign, Numeral},
    style::{DecimalMarker, DigitGroupSeparator, ExponentProduct, QuantityStyle},
    typeset::lowering::QuantityPiece,
  };

  /// 部分ごとの字面から数値を作る
  fn numeral(sign: Option<NumberSign>, integer: &str, fraction: Option<&str>, exponent: Option<&str>) -> Numeral {
    return Numeral {
      sign,
      integer: integer.to_string(),
      fraction: fraction.map(str::to_string),
      exponent: exponent.map(str::to_string),
    };
  }

  /// 整数部だけの数値
  fn int(integer: &str) -> Numeral { return numeral(None, integer, None, None); }

  /// 字の部品
  fn text(s: &str) -> QuantityPiece { return QuantityPiece::Text(s.to_string()); }

  /// 細アキの部品（桁区切り）
  const THIN: QuantityPiece = QuantityPiece::Space(MathSpace::Thin);

  /// 中アキの部品（積の記号の前後）
  const MEDIUM: QuantityPiece = QuantityPiece::Space(MathSpace::Medium);

  #[test]
  fn four_digit_integer_is_grouped_only_when_asked() {
    let grouped = QuantityStyle {
      group_four_digits: true,
      ..QuantityStyle::default()
    };

    assert_eq!(number_pieces(&int("1234"), &QuantityStyle::default()), [text("1234")]);
    assert_eq!(number_pieces(&int("1234"), &grouped), [text("1"), THIN, text("234")]);
    assert_eq!(number_pieces(&int("123"), &grouped), [text("123")]);
  }

  #[test]
  fn integer_groups_from_the_right() {
    let style = QuantityStyle::default();

    assert_eq!(number_pieces(&int("12345"), &style), [text("12"), THIN, text("345")]);
    assert_eq!(number_pieces(&int("1234567"), &style), [text("1"), THIN, text("234"), THIN, text("567")]);
  }

  #[test]
  fn fraction_groups_from_the_left_and_keeps_trailing_zero() {
    let style = QuantityStyle::default();

    assert_eq!(number_pieces(&numeral(None, "1", Some("23456"), None), &style), [text("1.234"), THIN, text("56")]);
    assert_eq!(number_pieces(&numeral(None, "1", Some("2345"), None), &style), [text("1.2345")]);
    assert_eq!(number_pieces(&numeral(None, "1", Some("50"), None), &style), [text("1.50")]);
  }

  #[test]
  fn separator_and_marker_follow_style() {
    let european = QuantityStyle {
      decimal_marker: DecimalMarker::Comma,
      digit_group_separator: DigitGroupSeparator::Period,
      ..QuantityStyle::default()
    };
    let ungrouped = QuantityStyle {
      digit_group_separator: DigitGroupSeparator::None,
      ..QuantityStyle::default()
    };
    let value = numeral(None, "12345", Some("678"), None);

    assert_eq!(number_pieces(&value, &european), [text("12.345,678")]);
    assert_eq!(number_pieces(&value, &ungrouped), [text("12345.678")]);
  }

  #[test]
  fn signs() {
    let style = QuantityStyle::default();

    assert_eq!(number_pieces(&numeral(Some(NumberSign::Minus), "5", None, None), &style), [text("\u{2212}5")]);
    assert_eq!(number_pieces(&numeral(Some(NumberSign::Plus), "5", None, None), &style), [text("+5")]);
  }

  #[test]
  fn exponent_takes_the_product_form() {
    let value = numeral(Some(NumberSign::Minus), "1", Some("5"), Some("-3"));
    let dot = QuantityStyle {
      exponent_product: ExponentProduct::Dot,
      ..QuantityStyle::default()
    };
    assert_eq!(
      number_pieces(&value, &QuantityStyle::default()),
      [
        text("\u{2212}1.5"),
        MEDIUM,
        text("\u{00D7}"),
        MEDIUM,
        text("10"),
        QuantityPiece::Superscript("\u{2212}3".to_string())
      ]
    );
    assert_eq!(number_pieces(&value, &dot)[2], text("\u{22C5}"));
  }

  #[test]
  fn plain_text_uses_unicode_spaces_and_superscripts() {
    let style = QuantityStyle::default();

    assert_eq!(plain_text(&number_pieces(&int("12345"), &style)), "12\u{202F}345");
    assert_eq!(
      plain_text(&number_pieces(&numeral(Some(NumberSign::Minus), "1", Some("5"), Some("-3")), &style)),
      "\u{2212}1.5\u{205F}\u{00D7}\u{205F}10\u{207B}\u{00B3}"
    );
    assert_eq!(
      plain_text(&number_pieces(&numeral(None, "2", None, Some("1234567890")), &style)),
      "2\u{205F}\u{00D7}\u{205F}10\u{00B9}\u{00B2}\u{00B3}\u{2074}\u{2075}\u{2076}\u{2077}\u{2078}\u{2079}\u{2070}"
    );
  }
}
