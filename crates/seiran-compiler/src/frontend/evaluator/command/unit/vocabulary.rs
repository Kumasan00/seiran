//! 単位の語彙 — SI 文書（第 9 版、2019。接頭辞は 2022 年の追加分を含む）の接頭辞・単位と `%`
//!
//! 接頭辞を付けられるのは SI 単位（°C を含む）と L l t eV Da Np B だけ。この規則の下で 1 因子の綴りはどれも一意に
//! 読める（`cd` は c·d ではなくカンデラ）ことを、表のテストが保つ。互換文字は自動では写さず、名指しした別名
//! （U+00B5 MICRO SIGN と U+2103 ℃）だけを受け付ける。

use icu_normalizer::ComposingNormalizerBorrowed;

use crate::{frontend::evaluator::EvalError, source::Span};

/// 接頭辞（q〜Q の 24 個と、micro の別名 U+00B5）
const PREFIXES: [&str; 25] = [
  "q", "r", "y", "z", "a", "f", "p", "n", "\u{03BC}", "\u{00B5}", "m", "c", "d", "da", "h", "k", "M", "G", "T", "P",
  "E", "Z", "Y", "R", "Q",
];

/// 接頭辞を付けられる単位（SI 単位と L l t Da eV Np B。U+2103 は °C の別名）
const PREFIXABLE_UNITS: [&str; 37] = [
  "m",
  "g",
  "s",
  "A",
  "K",
  "mol",
  "cd",
  "rad",
  "sr",
  "Hz",
  "N",
  "Pa",
  "J",
  "W",
  "C",
  "V",
  "F",
  "\u{03A9}",
  "S",
  "Wb",
  "T",
  "H",
  "\u{00B0}C",
  "\u{2103}",
  "lm",
  "lx",
  "Bq",
  "Gy",
  "Sv",
  "kat",
  "L",
  "l",
  "t",
  "Da",
  "eV",
  "Np",
  "B",
];

/// 接頭辞を付けられない単位
const UNPREFIXABLE_UNITS: [&str; 9] = [
  "min", "h", "d", "au", "\u{00B0}", "\u{2032}", "\u{2033}", "ha", "%",
];

/// 因子の記号 `symbol`（ソース上の `span`）が語彙で読めるかを確かめる（NFC にしてから引く）
///
/// # Errors
///
/// 接頭辞を付けられない単位に接頭辞が付いていれば `UnitPrefixNotAllowed`、語彙で読めなければ `UnknownUnit`。
pub(super) fn check(symbol: &str, span: Span) -> Result<(), EvalError> {
  let normalized = ComposingNormalizerBorrowed::new_nfc().normalize(symbol);
  if PREFIXABLE_UNITS.contains(&&*normalized) || UNPREFIXABLE_UNITS.contains(&&*normalized) {
    return Ok(());
  }
  let mut not_allowed = None;
  for prefix in PREFIXES {
    let Some(rest) = normalized.strip_prefix(prefix) else {
      continue;
    };
    if PREFIXABLE_UNITS.contains(&rest) {
      return Ok(());
    }
    if UNPREFIXABLE_UNITS.contains(&rest) {
      not_allowed = Some((prefix, rest.to_string()));
    }
  }
  return Err(match not_allowed {
    Some((prefix, unit)) => EvalError::UnitPrefixNotAllowed {
      span: span.into(),
      prefix: prefix.to_string(),
      unit,
    },
    None => EvalError::UnknownUnit {
      span: span.into(),
      symbol: symbol.to_string(),
    },
  });
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::{PREFIXABLE_UNITS, PREFIXES, UNPREFIXABLE_UNITS, check};
  use crate::{frontend::evaluator::EvalError, source::Span};

  /// 診断の位置（テストでは使わない）
  fn span() -> Span { return Span::new(0, 0); }

  #[test]
  fn every_unit_and_prefixed_unit_is_accepted() {
    for unit in PREFIXABLE_UNITS.iter().chain(&UNPREFIXABLE_UNITS) {
      assert!(check(unit, span()).is_ok(), "{unit}");
    }
    for prefix in PREFIXES {
      for unit in PREFIXABLE_UNITS {
        let symbol = format!("{prefix}{unit}");
        assert!(check(&symbol, span()).is_ok(), "{symbol}");
      }
    }
  }

  #[test]
  fn prefix_on_an_unprefixable_unit_is_rejected() {
    for prefix in PREFIXES {
      for unit in UNPREFIXABLE_UNITS {
        let symbol = format!("{prefix}{unit}");
        // 単位そのものの綴り（c + d の `cd` はカンデラ）は単位として読む
        if PREFIXABLE_UNITS.contains(&symbol.as_str()) {
          continue;
        }
        assert!(
          matches!(check(&symbol, span()), Err(EvalError::UnitPrefixNotAllowed { .. })),
          "{symbol}: {:?}",
          check(&symbol, span())
        );
      }
    }
    assert!(matches!(
      check("kmin", span()),
      Err(EvalError::UnitPrefixNotAllowed { ref prefix, ref unit, .. }) if prefix == "k" && unit == "min"
    ));
  }

  #[test]
  fn every_valid_spelling_has_one_reading() {
    // 単位そのものと「接頭辞 + 付けられる単位」の綴りが重なると、1 因子の読みが 2 通りになる
    let mut readings: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for unit in PREFIXABLE_UNITS.iter().chain(&UNPREFIXABLE_UNITS) {
      readings.entry((*unit).to_string()).or_default().push((*unit).to_string());
    }
    for prefix in PREFIXES {
      for unit in PREFIXABLE_UNITS {
        readings.entry(format!("{prefix}{unit}")).or_default().push(format!("{prefix} + {unit}"));
      }
    }

    let ambiguous: Vec<_> = readings.into_iter().filter(|(_, ways)| return ways.len() > 1).collect();
    assert!(ambiguous.is_empty(), "{ambiguous:?}");
  }

  #[test]
  fn representative_spellings() {
    for ok in [
      "cd",
      "Pa",
      "min",
      "ha",
      "kat",
      "mol",
      "Da",
      "dam",
      "dB",
      "kg",
      "m\u{00B0}C",
      "\u{00B5}m",
      "k\u{2103}",
      "\u{2126}",
      "\u{212A}",
    ] {
      assert!(check(ok, span()).is_ok(), "{ok}");
    }
    for prefixed in ["mha", "k%"] {
      assert!(matches!(check(prefixed, span()), Err(EvalError::UnitPrefixNotAllowed { .. })), "{prefixed}");
    }
    for unknown in ["bar", "metre", "N\u{00B7}m", "s\u{00B2}", "um", "ohm"] {
      assert!(matches!(check(unknown, span()), Err(EvalError::UnknownUnit { .. })), "{unknown}");
    }
  }
}
