//! Mathematical Alphanumeric Symbols へのコードポイント変換
//!
//! 数式中の 1 文字を、外側の [`MathStyle`] に応じて Unicode の
//! Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）と Greek ブロックへ
//! 変換するヘルパー群。
//!
//! 親モジュール [`super`]（`math.rs`）からのみ参照される内部モジュール。

use document::MathStyle;

/// 1 文字を `style` に応じた Mathematical Alphanumeric コードポイントへ変換する
///
/// - `style == None`: ASCII 英字のみ Mathematical Italic（小文字 `h` は U+210E PLANCK CONSTANT）。
///   数字・Greek・記号は素通し
/// - `style == Some(_)`: ASCII 英字・ASCII 数字・Greek 文字を該当スタイルのコードポイントに変換。
///   当該スタイル × 種別の組み合わせが Unicode に存在しない場合（例: `\mathmono` の Greek、
///   italic の数字）は素通しでフォント側のグリフ選択に委ねる
pub(super) fn translate_math_char(ch: char, style: Option<MathStyle>) -> char {
  // 共通ヘルパ: ASCII 英字を base に応じてシフト
  let map_ascii = |ch: char, upper_base: u32, lower_base: u32, italic_h_exception: bool| -> char {
    match ch {
      'A'..='Z' => char::from_u32(upper_base + (ch as u32 - 'A' as u32)).unwrap_or(ch),
      'h' if italic_h_exception => '\u{210E}',
      'a'..='z' => char::from_u32(lower_base + (ch as u32 - 'a' as u32)).unwrap_or(ch),
      _ => ch,
    }
  };

  // 共通ヘルパ: ASCII 数字を base に応じてシフト
  let map_digit = |ch: char, base: u32| -> char {
    match ch {
      '0'..='9' => char::from_u32(base + (ch as u32 - '0' as u32)).unwrap_or(ch),
      _ => ch,
    }
  };

  // 共通ヘルパ: Greek 文字を base に応じてシフト
  let map_greek = |ch: char, base: u32| -> char {
    match greek_math_offset(ch) {
      Some(offset) => char::from_u32(base + offset).unwrap_or(ch),
      None => ch,
    }
  };

  match style {
    None => {
      // デフォルト: ASCII 英字のみ Mathematical Italic、h 例外、Greek/digits は素通し
      return map_ascii(ch, 0x1d434, 0x1d44e, true);
    },
    Some(MathStyle::Serif) => return ch,
    Some(MathStyle::Italic) => {
      let mapped = map_ascii(ch, 0x1d434, 0x1d44e, true);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d6e2);
    },
    Some(MathStyle::Bold) => {
      let mapped = map_ascii(ch, 0x1d400, 0x1d41a, false);
      if mapped != ch {
        return mapped;
      }
      let mapped = map_digit(ch, 0x1d7ce);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d6a8);
    },
    Some(MathStyle::BoldItalic) => {
      let mapped = map_ascii(ch, 0x1d468, 0x1d482, false);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d71c);
    },
    Some(MathStyle::Sans) => {
      let mapped = map_ascii(ch, 0x1d5a0, 0x1d5ba, false);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7e2);
    },
    Some(MathStyle::SansItalic) => {
      return map_ascii(ch, 0x1d608, 0x1d622, false);
    },
    Some(MathStyle::SansBold) => {
      let mapped = map_ascii(ch, 0x1d5d4, 0x1d5ee, false);
      if mapped != ch {
        return mapped;
      }
      let mapped = map_digit(ch, 0x1d7ec);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d756);
    },
    Some(MathStyle::SansBoldItalic) => {
      let mapped = map_ascii(ch, 0x1d63c, 0x1d656, false);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d790);
    },
    Some(MathStyle::Mono) => {
      let mapped = map_ascii(ch, 0x1d670, 0x1d68a, false);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7f6);
    },
  }
}

/// Greek 文字 1 文字の Mathematical Greek block 内オフセットを返す
///
/// 大文字 Α..Ρ → 0..16, ϴ → 17, Σ..Ω → 18..24, ∇ → 25。
/// 小文字 α..ρ → 26..42, ς → 43, σ..ω → 44..50。
/// variants は ∂ → 51, ϵ → 52, ϑ → 53, ϰ → 54, ϕ → 55, ϱ → 56, ϖ → 57。
///
/// Unicode Mathematical Greek block（Bold: U+1D6A8〜 / Italic: U+1D6E2〜 等）は
/// すべて同じ 58 要素のレイアウトを持つので、base codepoint にこのオフセットを
/// 加えるだけで該当スタイルのコードポイントを得られる。
fn greek_math_offset(ch: char) -> Option<u32> {
  return match ch {
    // 大文字 Α..Ρ
    '\u{0391}' => Some(0),  // Α
    '\u{0392}' => Some(1),  // Β
    '\u{0393}' => Some(2),  // Γ
    '\u{0394}' => Some(3),  // Δ
    '\u{0395}' => Some(4),  // Ε
    '\u{0396}' => Some(5),  // Ζ
    '\u{0397}' => Some(6),  // Η
    '\u{0398}' => Some(7),  // Θ
    '\u{0399}' => Some(8),  // Ι
    '\u{039A}' => Some(9),  // Κ
    '\u{039B}' => Some(10), // Λ
    '\u{039C}' => Some(11), // Μ
    '\u{039D}' => Some(12), // Ν
    '\u{039E}' => Some(13), // Ξ
    '\u{039F}' => Some(14), // Ο
    '\u{03A0}' => Some(15), // Π
    '\u{03A1}' => Some(16), // Ρ
    // ϴ GREEK CAPITAL THETA SYMBOL
    '\u{03F4}' => Some(17),
    // Σ..Ω
    '\u{03A3}' => Some(18), // Σ
    '\u{03A4}' => Some(19), // Τ
    '\u{03A5}' => Some(20), // Υ
    '\u{03A6}' => Some(21), // Φ
    '\u{03A7}' => Some(22), // Χ
    '\u{03A8}' => Some(23), // Ψ
    '\u{03A9}' => Some(24), // Ω
    // ∇ NABLA
    '\u{2207}' => Some(25),
    // 小文字 α..ρ
    '\u{03B1}' => Some(26), // α
    '\u{03B2}' => Some(27), // β
    '\u{03B3}' => Some(28), // γ
    '\u{03B4}' => Some(29), // δ
    '\u{03B5}' => Some(30), // ε
    '\u{03B6}' => Some(31), // ζ
    '\u{03B7}' => Some(32), // η
    '\u{03B8}' => Some(33), // θ
    '\u{03B9}' => Some(34), // ι
    '\u{03BA}' => Some(35), // κ
    '\u{03BB}' => Some(36), // λ
    '\u{03BC}' => Some(37), // μ
    '\u{03BD}' => Some(38), // ν
    '\u{03BE}' => Some(39), // ξ
    '\u{03BF}' => Some(40), // ο
    '\u{03C0}' => Some(41), // π
    '\u{03C1}' => Some(42), // ρ
    // ς final sigma
    '\u{03C2}' => Some(43),
    // σ..ω
    '\u{03C3}' => Some(44), // σ
    '\u{03C4}' => Some(45), // τ
    '\u{03C5}' => Some(46), // υ
    '\u{03C6}' => Some(47), // φ
    '\u{03C7}' => Some(48), // χ
    '\u{03C8}' => Some(49), // ψ
    '\u{03C9}' => Some(50), // ω
    // variants
    '\u{2202}' => Some(51), // ∂ PARTIAL DIFFERENTIAL
    '\u{03F5}' => Some(52), // ϵ GREEK LUNATE EPSILON SYMBOL
    '\u{03D1}' => Some(53), // ϑ GREEK THETA SYMBOL
    '\u{03F0}' => Some(54), // ϰ GREEK KAPPA SYMBOL
    '\u{03D5}' => Some(55), // ϕ GREEK PHI SYMBOL
    '\u{03F1}' => Some(56), // ϱ GREEK RHO SYMBOL
    '\u{03D6}' => Some(57), // ϖ GREEK PI SYMBOL
    _ => None,
  };
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn translate_default_italicizes_ascii_letters_only() {
    // Arrange & Act & Assert — デフォルト: ASCII 英字のみ italic 化、他は素通し
    assert_eq!(translate_math_char('a', None), '\u{1D44E}'); // mathematical italic a
    assert_eq!(translate_math_char('z', None), '\u{1D467}'); // mathematical italic z
    assert_eq!(translate_math_char('A', None), '\u{1D434}'); // mathematical italic A
    assert_eq!(translate_math_char('Z', None), '\u{1D44D}'); // mathematical italic Z
    assert_eq!(translate_math_char('h', None), '\u{210E}'); // PLANCK CONSTANT (h 例外)
    assert_eq!(translate_math_char('1', None), '1', "デフォルトでは数字は素通し");
    assert_eq!(translate_math_char('+', None), '+', "記号は素通し");
    assert_eq!(translate_math_char('α', None), 'α', "Greek もデフォルトは素通し");
  }

  #[test]
  fn translate_bold_covers_letters_digits_greek() {
    // Arrange & Act & Assert
    let style = Some(MathStyle::Bold);
    assert_eq!(translate_math_char('x', style), '\u{1D431}'); // mathematical bold x
    assert_eq!(translate_math_char('A', style), '\u{1D400}'); // mathematical bold A
    assert_eq!(translate_math_char('0', style), '\u{1D7CE}'); // mathematical bold digit 0
    assert_eq!(translate_math_char('9', style), '\u{1D7D7}'); // mathematical bold digit 9
    assert_eq!(translate_math_char('α', style), '\u{1D6C2}'); // mathematical bold alpha
    assert_eq!(translate_math_char('Ω', style), '\u{1D6C0}'); // mathematical bold Omega
    assert_eq!(translate_math_char('+', style), '+', "記号は素通し");
  }

  #[test]
  fn translate_italic_h_uses_planck_constant_exception() {
    // Arrange & Act & Assert — italic 系のみ h の例外
    assert_eq!(translate_math_char('h', Some(MathStyle::Italic)), '\u{210E}');
    assert_eq!(
      translate_math_char('h', Some(MathStyle::BoldItalic)),
      '\u{1D489}',
      "BoldItalic h は通常コードポイント"
    );
  }

  #[test]
  fn translate_mono_keeps_greek_passthrough() {
    // Arrange & Act & Assert — Mono は Greek 未対応 → 素通し
    assert_eq!(translate_math_char('α', Some(MathStyle::Mono)), 'α');
    assert_eq!(translate_math_char('a', Some(MathStyle::Mono)), '\u{1D68A}'); // mono a
    assert_eq!(translate_math_char('5', Some(MathStyle::Mono)), '\u{1D7FB}'); // mono digit 5
  }

  #[test]
  fn translate_serif_is_passthrough() {
    // Arrange & Act & Assert — Serif (Roman) は全部素通し
    let style = Some(MathStyle::Serif);
    assert_eq!(translate_math_char('x', style), 'x');
    assert_eq!(translate_math_char('1', style), '1');
    assert_eq!(translate_math_char('α', style), 'α');
  }

  #[test]
  fn translate_sans_skips_greek() {
    // Arrange & Act & Assert — Sans は Greek 未対応 (Unicode に該当コードポイントなし)
    let style = Some(MathStyle::Sans);
    assert_eq!(translate_math_char('a', style), '\u{1D5BA}'); // sans a
    assert_eq!(translate_math_char('1', style), '\u{1D7E3}'); // sans digit 1
    assert_eq!(translate_math_char('α', style), 'α', "Sans Greek は Unicode 未定義のため素通し");
  }
}
