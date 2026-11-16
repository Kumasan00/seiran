//! 数値（`document::Numeral`）・単位（`document::UnitFactor`）・量（その 2 つの組）を style.toml `[quantity]` の書式の部品列（[`QuantityPiece`]）に整える
//!
//! 部品列は本文・数式の箱（boxing）と PDF しおりの平文（[`plain_text`]）の両方の元になる。

use crate::{
  document::{MathSpace, NumberSign, Numeral, UnitFactor},
  style::{
    DecimalMarker, DigitGroupSeparator, ExponentProduct, NegativeExponent, NumberUnitSeparator, QuantityStyle,
    UnitProduct,
  },
  typeset::lowering::{QuantityPiece, math},
};

/// 桁区切りを入れない最大の桁数（4 桁は `group_four_digits` のときだけ区切る）
const UNGROUPED_DIGITS: usize = 3;

/// 数値と単位の間を空けない平面角の単位（SI 文書 第 9 版 5.4.3 の、数値と単位の間は空けるという規則の例外）
const PLANE_ANGLE_SYMBOLS: [&str; 3] = ["\u{00B0}", "\u{2032}", "\u{2033}"];

/// 数値を `style` の書式の部品列にする（隣り合う字は 1 つの [`QuantityPiece::Text`] に畳む）
///
/// 負号は U+2212。指数があれば `仮数 × 10^指数` の形にし、積の記号の前後は中アキ。
pub(in crate::typeset) fn number_pieces(numeral: &Numeral, style: &QuantityStyle) -> Vec<QuantityPiece> {
  let mut pieces = Vec::new();
  push_integer(&mut pieces, numeral, style);
  push_after_integer(&mut pieces, numeral, style);
  return pieces;
}

/// 数値を符号 + 整数部（桁区切り込み）の左と、小数点 + 小数部 + 指数の右に分ける
///
/// 分けるのは数値の構造（`Numeral` の部分）で、出力した字は探さない（`decimal_marker = comma` と
/// `digit_group_separator = period` の組でも `.` は左に入る）。つなぐと [`number_pieces`] と同じ字になる。
pub(in crate::typeset) fn split_number_pieces(
  numeral: &Numeral,
  style: &QuantityStyle,
) -> (Vec<QuantityPiece>, Vec<QuantityPiece>) {
  let mut left = Vec::new();
  push_integer(&mut left, numeral, style);
  let mut right = Vec::new();
  push_after_integer(&mut right, numeral, style);
  return (left, right);
}

/// 量を [`split_number_pieces`] と同じ左と、数値の残り + 数値と単位の間のアキ + 単位の右に分ける
///
/// アキは [`quantity_pieces`] と同じ規則（平面角の例外を含む）。つなぐと [`quantity_pieces`] と同じ字になる。
pub(in crate::typeset) fn split_quantity_pieces(
  numeral: &Numeral,
  unit: &[UnitFactor],
  style: &QuantityStyle,
) -> (Vec<QuantityPiece>, Vec<QuantityPiece>) {
  let (left, mut right) = split_number_pieces(numeral, style);
  push_number_unit_gap(&mut right, unit, style);
  push_unit(&mut right, unit, style);
  return (left, right);
}

/// 単位の式を `style` の表記の部品列にする（指数は負号を U+2212 にした上付き）
///
/// 積は `unit_product`（細アキか、前後のアキなしの U+22C5）。`negative_exponent = slash` で正と負の指数の因子が両方あれば
/// `分子 / 分母` の形にし（分母は符号を落とし、`-1` は指数なし。分母が 2 因子以上なら括弧で包む）、因子の相対順は保つ。
pub(in crate::typeset) fn unit_pieces(factors: &[UnitFactor], style: &QuantityStyle) -> Vec<QuantityPiece> {
  let mut pieces = Vec::new();
  push_unit(&mut pieces, factors, style);
  return pieces;
}

/// 量（数値と単位の式）を `style` の部品列にする（数値・単位の部分はそれぞれ [`number_pieces`] / [`unit_pieces`] と同じ）
///
/// 間のアキは `number_unit_separator`。単位の式が平面角 ° ′ ″ の 1 因子だけ（指数なし）なら、style によらずアキを置かない。
/// アキが無ければ数値の末尾の字と単位の先頭の字は 1 つの [`QuantityPiece::Text`] に畳む。
pub(in crate::typeset) fn quantity_pieces(
  numeral: &Numeral,
  unit: &[UnitFactor],
  style: &QuantityStyle,
) -> Vec<QuantityPiece> {
  let mut pieces = Vec::new();
  push_integer(&mut pieces, numeral, style);
  push_after_integer(&mut pieces, numeral, style);
  push_number_unit_gap(&mut pieces, unit, style);
  push_unit(&mut pieces, unit, style);
  return pieces;
}

/// 数値と単位の間の `number_unit_separator` のアキを部品列 `pieces` に積む（単位が平面角 ° ′ ″ の 1 因子だけなら積まない）
fn push_number_unit_gap(pieces: &mut Vec<QuantityPiece>, unit: &[UnitFactor], style: &QuantityStyle) {
  let plane_angle =
    matches!(unit, [UnitFactor { symbol, exponent: None }] if PLANE_ANGLE_SYMBOLS.contains(&symbol.as_str()));
  if plane_angle {
    return;
  }
  match style.number_unit_separator {
    NumberUnitSeparator::None => {},
    NumberUnitSeparator::Thin => pieces.push(QuantityPiece::Space(MathSpace::Thin)),
    NumberUnitSeparator::Medium => pieces.push(QuantityPiece::Space(MathSpace::Medium)),
    NumberUnitSeparator::Thick => pieces.push(QuantityPiece::Space(MathSpace::Thick)),
  }
}

/// 数値の符号と整数部（桁区切り込み）を部品列 `pieces` に積む
fn push_integer(pieces: &mut Vec<QuantityPiece>, numeral: &Numeral, style: &QuantityStyle) {
  match numeral.sign {
    Some(NumberSign::Plus) => push_text(pieces, "+"),
    Some(NumberSign::Minus) => push_text(pieces, "\u{2212}"),
    None => {},
  }
  push_grouped(pieces, &numeral.integer, true, style);
}

/// 数値の小数点・小数部・指数（整数部より後ろ）を部品列 `pieces` に積む
fn push_after_integer(pieces: &mut Vec<QuantityPiece>, numeral: &Numeral, style: &QuantityStyle) {
  if let Some(fraction) = &numeral.fraction {
    push_text(
      pieces,
      match style.decimal_marker {
        DecimalMarker::Period => ".",
        DecimalMarker::Comma => ",",
      },
    );
    push_grouped(pieces, fraction, false, style);
  }
  if let Some(exponent) = &numeral.exponent {
    pieces.push(QuantityPiece::Space(MathSpace::Medium));
    push_text(
      pieces,
      match style.exponent_product {
        ExponentProduct::Times => "\u{00D7}",
        ExponentProduct::Dot => "\u{22C5}",
      },
    );
    pieces.push(QuantityPiece::Space(MathSpace::Medium));
    push_text(pieces, "10");
    pieces.push(QuantityPiece::Superscript(exponent.replace('-', "\u{2212}")));
  }
}

/// 単位の式を `style` の表記で部品列 `pieces` に積む
fn push_unit(pieces: &mut Vec<QuantityPiece>, factors: &[UnitFactor], style: &QuantityStyle) {
  let (denominator, numerator): (Vec<&UnitFactor>, Vec<&UnitFactor>) = factors
    .iter()
    .partition(|factor| return factor.exponent.as_deref().is_some_and(|e| return e.starts_with('-')));
  if style.negative_exponent == NegativeExponent::Power || numerator.is_empty() || denominator.is_empty() {
    push_unit_product(pieces, factors.iter().map(written), style.unit_product);
    return;
  }
  push_unit_product(pieces, numerator.into_iter().map(written), style.unit_product);
  push_text(pieces, "/");
  let parenthesized = denominator.len() > 1;
  if parenthesized {
    push_text(pieces, "(");
  }
  push_unit_product(pieces, denominator.into_iter().map(unsigned), style.unit_product);
  if parenthesized {
    push_text(pieces, ")");
  }
}

/// 因子の（記号, 書いたままの指数）
fn written(factor: &UnitFactor) -> (&str, Option<&str>) { return (factor.symbol.as_str(), factor.exponent.as_deref()); }

/// 分母に置く負の指数の因子の（記号, 符号を落とした指数。`-1` は指数なし）
fn unsigned(factor: &UnitFactor) -> (&str, Option<&str>) {
  let magnitude = factor.exponent.as_deref().and_then(|exponent| return exponent.strip_prefix('-'));
  return (factor.symbol.as_str(), magnitude.filter(|&magnitude| return magnitude != "1"));
}

/// （記号, 指数）の列を積の記号 `product` で並べて積む
fn push_unit_product<'a>(
  pieces: &mut Vec<QuantityPiece>,
  factors: impl Iterator<Item = (&'a str, Option<&'a str>)>,
  product: UnitProduct,
) {
  for (index, (symbol, exponent)) in factors.enumerate() {
    if index > 0 {
      match product {
        UnitProduct::Thin => pieces.push(QuantityPiece::Space(MathSpace::Thin)),
        UnitProduct::Dot => push_text(pieces, "\u{22C5}"),
      }
    }
    push_text(pieces, symbol);
    if let Some(exponent) = exponent {
      pieces.push(QuantityPiece::Superscript(exponent.replace('-', "\u{2212}")));
    }
  }
}

/// 部品列を PDF しおりの平文にする（細アキ U+202F・中アキ U+205F・太アキ U+2004・上付きは Unicode の上付き字）
pub(super) fn plain_text(pieces: &[QuantityPiece]) -> String {
  let mut out = String::new();
  for piece in pieces {
    match piece {
      QuantityPiece::Text(text) => out.push_str(text),
      QuantityPiece::Space(MathSpace::Thin) => out.push('\u{202F}'),
      QuantityPiece::Space(MathSpace::Medium) => out.push('\u{205F}'),
      QuantityPiece::Space(MathSpace::Thick) => out.push('\u{2004}'),
      QuantityPiece::Space(MathSpace::Quad | MathSpace::NegativeThin) => {
        unreachable!(
          "部品列のアキは細アキ（桁区切り・単位の積）・中アキ（積の記号の前後）と、数値と単位の間の細・中・太アキだけ"
        )
      },
      QuantityPiece::Superscript(text) => out.extend(text.chars().map(|ch| {
        return math::superscript_char(ch).expect(
          "指数の字は frontend の数値・単位の文法を通った ASCII 数字と、push_after_integer / push_unit が負号にした U+2212 だけで、どれも上付き字の表にある",
        );
      })),
    }
  }
  return out;
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
  let long_enough = len > UNGROUPED_DIGITS + 1 || (len == UNGROUPED_DIGITS + 1 && style.group_four_digits);
  let separator: Option<fn(&mut Vec<QuantityPiece>)> = match style.digit_group_separator {
    DigitGroupSeparator::None => None,
    DigitGroupSeparator::Thin => Some(|pieces| pieces.push(QuantityPiece::Space(MathSpace::Thin))),
    DigitGroupSeparator::Comma => Some(|pieces| push_text(pieces, ",")),
    DigitGroupSeparator::Period => Some(|pieces| push_text(pieces, ".")),
  };
  let Some(push_separator) = separator.filter(|_| return long_enough) else {
    push_text(pieces, digits);
    return;
  };
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
      push_separator(pieces);
    }
    push_text(pieces, &digits[start..end]);
    start = end;
    end = (end + UNGROUPED_DIGITS).min(len);
  }
}

#[cfg(test)]
mod tests {
  use super::{number_pieces, plain_text, quantity_pieces, split_number_pieces, split_quantity_pieces, unit_pieces};
  use crate::{
    document::{MathSpace, NumberSign, Numeral, UnitFactor},
    style::{
      DecimalMarker, DigitGroupSeparator, ExponentProduct, NegativeExponent, NumberUnitSeparator, QuantityStyle,
      UnitProduct,
    },
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
  fn comma_separator_joins_the_digit_text() {
    let style = QuantityStyle {
      digit_group_separator: DigitGroupSeparator::Comma,
      ..QuantityStyle::default()
    };

    assert_eq!(number_pieces(&int("1234567"), &style), [text("1,234,567")]);
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

  /// 字面から単位の因子を作る
  fn factor(symbol: &str, exponent: Option<&str>) -> UnitFactor {
    return UnitFactor {
      symbol: symbol.to_string(),
      exponent: exponent.map(str::to_string),
    };
  }

  /// 上付きの部品
  fn sup(s: &str) -> QuantityPiece { return QuantityPiece::Superscript(s.to_string()); }

  /// 積の記号と負の指数の表記を指定した style
  fn unit_style(unit_product: UnitProduct, negative_exponent: NegativeExponent) -> QuantityStyle {
    return QuantityStyle {
      unit_product,
      negative_exponent,
      ..QuantityStyle::default()
    };
  }

  #[test]
  fn unit_pieces_power_form() {
    let pieces = unit_pieces(&[factor("km", None), factor("s", Some("-2"))], &QuantityStyle::default());

    assert_eq!(
      pieces,
      [
        text("km"),
        THIN,
        text("s"),
        QuantityPiece::Superscript("\u{2212}2".to_string())
      ]
    );
    assert_eq!(plain_text(&pieces), "km\u{202F}s\u{207B}\u{00B2}");
  }

  #[test]
  fn dot_product_has_no_space() {
    let dot = unit_style(UnitProduct::Dot, NegativeExponent::Power);

    assert_eq!(unit_pieces(&[factor("N", None), factor("m", None)], &dot), [text("N\u{22C5}m")]);
  }

  #[test]
  fn power_keeps_the_written_order() {
    let pieces = unit_pieces(&[factor("s", Some("-1")), factor("m", None)], &QuantityStyle::default());

    assert_eq!(pieces, [text("s"), sup("\u{2212}1"), THIN, text("m")]);
  }

  #[test]
  fn slash_moves_the_denominator_after_the_numerator() {
    let slash = unit_style(UnitProduct::Thin, NegativeExponent::Slash);

    assert_eq!(unit_pieces(&[factor("m", None), factor("s", Some("-2"))], &slash), [text("m/s"), sup("2")]);
    assert_eq!(unit_pieces(&[factor("s", Some("-1")), factor("m", None)], &slash), [text("m/s")]);
    assert_eq!(
      unit_pieces(&[factor("m", Some("2")), factor("s", Some("-1"))], &slash),
      [text("m"), sup("2"), text("/s")]
    );
  }

  #[test]
  fn slash_parenthesizes_two_or_more_denominators() {
    let factors = [
      factor("J", None),
      factor("kg", Some("-1")),
      factor("K", Some("-1")),
    ];

    let dot = unit_pieces(&factors, &unit_style(UnitProduct::Dot, NegativeExponent::Slash));
    let thin = unit_pieces(&factors, &unit_style(UnitProduct::Thin, NegativeExponent::Slash));

    assert_eq!(dot, [text("J/(kg\u{22C5}K)")]);
    assert_eq!(thin, [text("J/(kg"), THIN, text("K)")]);
    assert_eq!(plain_text(&dot), "J/(kg\u{22C5}K)");
  }

  #[test]
  fn slash_without_a_positive_factor_keeps_the_power_form() {
    let slash = unit_style(UnitProduct::Thin, NegativeExponent::Slash);

    assert_eq!(unit_pieces(&[factor("s", Some("-1"))], &slash), [text("s"), sup("\u{2212}1")]);
  }

  /// 数値と単位の間のアキを指定した style
  fn separated(number_unit_separator: NumberUnitSeparator) -> QuantityStyle {
    return QuantityStyle {
      number_unit_separator,
      ..QuantityStyle::default()
    };
  }

  #[test]
  fn separator_follows_style() {
    let metre = [factor("m", None)];
    let pieces = |separator| return quantity_pieces(&int("5"), &metre, &separated(separator));

    assert_eq!(pieces(NumberUnitSeparator::None), [text("5m")]);
    assert_eq!(pieces(NumberUnitSeparator::Thin), [text("5"), THIN, text("m")]);
    assert_eq!(pieces(NumberUnitSeparator::Medium), [text("5"), MEDIUM, text("m")]);
    assert_eq!(pieces(NumberUnitSeparator::Thick), [text("5"), QuantityPiece::Space(MathSpace::Thick), text("m")]);
  }

  #[test]
  fn plane_angle_alone_has_no_space() {
    let thick = separated(NumberUnitSeparator::Thick);

    for symbol in ["\u{00B0}", "\u{2032}", "\u{2033}"] {
      let unit = [factor(symbol, None)];
      assert_eq!(quantity_pieces(&int("30"), &unit, &QuantityStyle::default()), [text(&format!("30{symbol}"))]);
      assert_eq!(quantity_pieces(&int("30"), &unit, &thick), [text(&format!("30{symbol}"))]);
    }
    let spaced = [
      vec![factor("\u{00B0}C", None)],
      vec![factor("%", None)],
      vec![factor("\u{00B0}", Some("2"))],
      vec![factor("\u{00B0}", None), factor("s", Some("-1"))],
    ];
    for unit in spaced {
      assert_eq!(quantity_pieces(&int("30"), &unit, &QuantityStyle::default())[1], THIN, "{unit:?}");
    }
  }

  #[test]
  fn number_exponent_and_unit_superscript_coexist() {
    let pieces =
      quantity_pieces(&numeral(None, "1", Some("5"), Some("3")), &[factor("m", Some("2"))], &QuantityStyle::default());

    assert_eq!(
      pieces,
      [
        text("1.5"),
        MEDIUM,
        text("\u{00D7}"),
        MEDIUM,
        text("10"),
        sup("3"),
        THIN,
        text("m"),
        sup("2")
      ]
    );
  }

  #[test]
  fn slash_without_positive_factor_stays_power() {
    let slash = unit_style(UnitProduct::Thin, NegativeExponent::Slash);

    assert_eq!(
      quantity_pieces(&int("5"), &[factor("s", Some("-1"))], &slash),
      [text("5"), THIN, text("s"), sup("\u{2212}1")]
    );
  }

  #[test]
  fn plain_text_maps_every_separator() {
    let metre = [factor("m", None)];
    let plain = |separator| return plain_text(&quantity_pieces(&int("5"), &metre, &separated(separator)));

    assert_eq!(plain(NumberUnitSeparator::None), "5m");
    assert_eq!(plain(NumberUnitSeparator::Thin), "5\u{202F}m");
    assert_eq!(plain(NumberUnitSeparator::Medium), "5\u{205F}m");
    assert_eq!(plain(NumberUnitSeparator::Thick), "5\u{2004}m");
  }

  /// L と R をつなぎ、隣り合う字を 1 つに畳む
  fn joined(left: Vec<QuantityPiece>, right: Vec<QuantityPiece>) -> Vec<QuantityPiece> {
    let mut pieces: Vec<QuantityPiece> = Vec::new();
    for piece in left.into_iter().chain(right) {
      match (pieces.last_mut(), piece) {
        (Some(QuantityPiece::Text(last)), QuantityPiece::Text(next)) => last.push_str(&next),
        (_, piece) => pieces.push(piece),
      }
    }
    return pieces;
  }

  #[test]
  fn split_number_pieces_puts_sign_and_integer_on_the_left() {
    let value = numeral(Some(NumberSign::Minus), "1234", Some("5"), Some("-3"));

    let grouped = QuantityStyle {
      group_four_digits: true,
      ..QuantityStyle::default()
    };

    let (left, right) = split_number_pieces(&value, &grouped);

    assert_eq!(left, [text("\u{2212}1"), THIN, text("234")]);
    assert_eq!(
      right,
      [
        text(".5"),
        MEDIUM,
        text("\u{00D7}"),
        MEDIUM,
        text("10"),
        sup("\u{2212}3")
      ]
    );
  }

  #[test]
  fn split_number_pieces_of_an_integer_has_an_empty_right() {
    let (left, right) = split_number_pieces(&int("42"), &QuantityStyle::default());

    assert_eq!(left, [text("42")]);
    assert_eq!(right, []);
  }

  #[test]
  fn split_number_pieces_splits_by_structure_not_by_glyph() {
    let european = QuantityStyle {
      decimal_marker: DecimalMarker::Comma,
      digit_group_separator: DigitGroupSeparator::Period,
      ..QuantityStyle::default()
    };

    let (left, right) = split_number_pieces(&numeral(None, "1234567", Some("5"), None), &european);

    assert_eq!(left, [text("1.234.567")]);
    assert_eq!(right, [text(",5")]);
  }

  #[test]
  fn split_pieces_join_back_to_the_unsplit_pieces() {
    let style = QuantityStyle::default();
    let european = QuantityStyle {
      decimal_marker: DecimalMarker::Comma,
      digit_group_separator: DigitGroupSeparator::Period,
      ..QuantityStyle::default()
    };
    let numerals = [
      numeral(Some(NumberSign::Minus), "1234", Some("5"), Some("-3")),
      int("42"),
      numeral(None, "1234567", Some("5"), None),
      numeral(None, "1", None, Some("3")),
      numeral(Some(NumberSign::Plus), "0", Some("5"), None),
    ];
    for value in &numerals {
      for style in [&style, &european] {
        let (left, right) = split_number_pieces(value, style);
        assert_eq!(joined(left, right), number_pieces(value, style), "{value:?}");
      }
    }
    let quantities = [
      (numeral(None, "9", Some("81"), None), vec![factor("m", None), factor("s", Some("-2"))]),
      (int("30"), vec![factor("\u{00B0}", None)]),
    ];
    for (value, unit) in &quantities {
      let (left, right) = split_quantity_pieces(value, unit, &style);
      assert_eq!(joined(left, right), quantity_pieces(value, unit, &style), "{value:?} {unit:?}");
    }
    let slash = QuantityStyle {
      negative_exponent: NegativeExponent::Slash,
      ..QuantityStyle::default()
    };
    let unit = [factor("m", None), factor("s", Some("-2"))];
    let (left, right) = split_quantity_pieces(&numeral(None, "9", Some("81"), None), &unit, &slash);
    assert_eq!(left, [text("9")]);
    assert!(right.contains(&text("m/s")) || right.contains(&sup("2")), "右側にスラッシュ形の単位: {right:?}");
    assert_eq!(
      joined(left, right),
      quantity_pieces(&numeral(None, "9", Some("81"), None), &unit, &slash),
      "スラッシュ形でも連結すると分けない場合と一致する"
    );
  }

  #[test]
  fn split_quantity_pieces_puts_the_unit_on_the_right() {
    let (left, right) =
      split_quantity_pieces(&numeral(None, "9", Some("81"), None), &[factor("m", None)], &QuantityStyle::default());

    assert_eq!(left, [text("9")]);
    assert_eq!(right, [text(".81"), THIN, text("m")]);
  }
}
