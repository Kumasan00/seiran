//! Mathematical Alphanumeric Symbols へのコードポイント変換

use crate::model::MathStyle;

/// 1 文字を `style` に応じた Mathematical Alphanumeric コードポイントへ変換する
pub(super) fn translate_math_char(ch: char, style: Option<MathStyle>) -> char {
  // `hole` は連続ブロックではなく Letterlike Symbols ブロックに散在する文字
  // （例: 黒板太字の ℝ=U+211D、italic の h=U+210E）を上書きするためのルックアップ。
  // 該当文字がなければ `None` を返し、通常の base + offset 変換を行う。
  let map_ascii = |ch: char, upper_base: u32, lower_base: u32, hole: fn(char) -> Option<char>| -> char {
    if let Some(mapped) = hole(ch) {
      return mapped;
    }
    match ch {
      'A'..='Z' => return char::from_u32(upper_base + (ch as u32 - 'A' as u32)).unwrap_or(ch),
      'a'..='z' => return char::from_u32(lower_base + (ch as u32 - 'a' as u32)).unwrap_or(ch),
      _ => return ch,
    }
  };

  let map_digit = |ch: char, base: u32| -> char {
    match ch {
      '0'..='9' => return char::from_u32(base + (ch as u32 - '0' as u32)).unwrap_or(ch),
      _ => return ch,
    }
  };

  let map_greek = |ch: char, base: u32| -> char {
    match greek_math_offset(ch) {
      Some(offset) => return char::from_u32(base + offset).unwrap_or(ch),
      None => return ch,
    }
  };

  match style {
    None => {
      // デフォルト: ASCII 英字のみ Mathematical Italic、h 例外、Greek/digits は素通し
      return map_ascii(ch, 0x1d434, 0x1d44e, italic_hole);
    },
    Some(MathStyle::Serif) => return ch,
    Some(MathStyle::Italic) => {
      let mapped = map_ascii(ch, 0x1d434, 0x1d44e, italic_hole);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d6e2);
    },
    Some(MathStyle::Bold) => {
      let mapped = map_ascii(ch, 0x1d400, 0x1d41a, no_hole);
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
      let mapped = map_ascii(ch, 0x1d468, 0x1d482, no_hole);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d71c);
    },
    Some(MathStyle::Sans) => {
      let mapped = map_ascii(ch, 0x1d5a0, 0x1d5ba, no_hole);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7e2);
    },
    Some(MathStyle::SansItalic) => {
      return map_ascii(ch, 0x1d608, 0x1d622, no_hole);
    },
    Some(MathStyle::SansBold) => {
      let mapped = map_ascii(ch, 0x1d5d4, 0x1d5ee, no_hole);
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
      let mapped = map_ascii(ch, 0x1d63c, 0x1d656, no_hole);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d790);
    },
    Some(MathStyle::Mono) => {
      let mapped = map_ascii(ch, 0x1d670, 0x1d68a, no_hole);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7f6);
    },
    Some(MathStyle::DoubleStruck) => {
      // 黒板太字: 大文字に 7 個の穴（ℂ ℍ ℕ ℙ ℚ ℝ ℤ）。小文字・数字は連続。Greek は Unicode 未定義
      let mapped = map_ascii(ch, 0x1d538, 0x1d552, double_struck_hole);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7d8);
    },
    Some(MathStyle::Script | MathStyle::Calligraphic) => {
      // スクリプト（roundhand）とカリグラフィー（chancery）は同一の基底コードポイントを共有する
      // （大文字に 8 個・小文字に 3 個の穴。数字・Greek は素通し）。両者の字形差はここでは付けず、
      // カリグラフィーのみ呼び出し側 `push_math_char` が異体字セレクタ VS1（U+FE00）を付与して
      // chancery 字形を選ぶ（VS1 はフォント依存。非対応フォントではスクリプト字形にフォールバック）。
      return map_ascii(ch, 0x1d49c, 0x1d4b6, script_hole);
    },
    Some(MathStyle::Fraktur) => {
      // フラクトゥール: 大文字に 5 個の穴（ℭ ℌ ℑ ℜ ℨ）。小文字は連続。数字・Greek は素通し
      return map_ascii(ch, 0x1d504, 0x1d51e, fraktur_hole);
    },
    Some(MathStyle::ScriptBold) => {
      // 太字スクリプト: 大文字・小文字とも連続（穴なし）。数字・Greek は Unicode 未定義のため素通し
      return map_ascii(ch, 0x1d4d0, 0x1d4ea, no_hole);
    },
    Some(MathStyle::FrakturBold) => {
      // 太字フラクトゥール: 大文字・小文字とも連続（穴なし）。数字・Greek は素通し
      return map_ascii(ch, 0x1d56c, 0x1d586, no_hole);
    },
  }
}

/// 1 文字を `style` に応じて変換し、必要なら異体字セレクタを付けて `out` に書き込む
pub(super) fn push_math_char(out: &mut String, ch: char, style: Option<MathStyle>) {
  out.push(translate_math_char(ch, style));
  if style == Some(MathStyle::Calligraphic) && ch.is_ascii_alphabetic() {
    out.push('\u{FE00}');
  }
}

/// 穴なし字体用のルックアップ（常に `None`）
fn no_hole(_ch: char) -> Option<char> { return None; }

/// Mathematical Italic の穴: 小文字 `h` は U+210E PLANCK CONSTANT
fn italic_hole(ch: char) -> Option<char> {
  return match ch {
    'h' => Some('\u{210E}'),
    _ => None,
  };
}

/// Mathematical Double-Struck（黒板太字）の穴
fn double_struck_hole(ch: char) -> Option<char> {
  return match ch {
    'C' => Some('\u{2102}'), // ℂ
    'H' => Some('\u{210D}'), // ℍ
    'N' => Some('\u{2115}'), // ℕ
    'P' => Some('\u{2119}'), // ℙ
    'Q' => Some('\u{211A}'), // ℚ
    'R' => Some('\u{211D}'), // ℝ
    'Z' => Some('\u{2124}'), // ℤ
    _ => None,
  };
}

/// Mathematical Script（スクリプト / カリグラフィー共用の基底）の穴
fn script_hole(ch: char) -> Option<char> {
  return match ch {
    'B' => Some('\u{212C}'), // ℬ
    'E' => Some('\u{2130}'), // ℰ
    'F' => Some('\u{2131}'), // ℱ
    'H' => Some('\u{210B}'), // ℋ
    'I' => Some('\u{2110}'), // ℐ
    'L' => Some('\u{2112}'), // ℒ
    'M' => Some('\u{2133}'), // ℳ
    'R' => Some('\u{211B}'), // ℛ
    'e' => Some('\u{212F}'), // ℯ
    'g' => Some('\u{210A}'), // ℊ
    'o' => Some('\u{2134}'), // ℴ
    _ => None,
  };
}

/// Mathematical Fraktur（フラクトゥール）の穴
fn fraktur_hole(ch: char) -> Option<char> {
  return match ch {
    'C' => Some('\u{212D}'), // ℭ
    'H' => Some('\u{210C}'), // ℌ
    'I' => Some('\u{2111}'), // ℑ
    'R' => Some('\u{211C}'), // ℜ
    'Z' => Some('\u{2128}'), // ℨ
    _ => None,
  };
}

/// Greek 文字 1 文字の Mathematical Greek block 内オフセットを返す
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
    // Arrange & Act
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
    // Arrange & Act
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
    // Arrange & Act
    assert_eq!(translate_math_char('h', Some(MathStyle::Italic)), '\u{210E}');
    assert_eq!(
      translate_math_char('h', Some(MathStyle::BoldItalic)),
      '\u{1D489}',
      "BoldItalic h は通常コードポイント"
    );
  }

  #[test]
  fn translate_mono_keeps_greek_passthrough() {
    // Arrange & Act
    assert_eq!(translate_math_char('α', Some(MathStyle::Mono)), 'α');
    assert_eq!(translate_math_char('a', Some(MathStyle::Mono)), '\u{1D68A}'); // mono a
    assert_eq!(translate_math_char('5', Some(MathStyle::Mono)), '\u{1D7FB}'); // mono digit 5
  }

  #[test]
  fn translate_serif_is_passthrough() {
    // Arrange & Act
    let style = Some(MathStyle::Serif);
    assert_eq!(translate_math_char('x', style), 'x');
    assert_eq!(translate_math_char('1', style), '1');
    assert_eq!(translate_math_char('α', style), 'α');
  }

  #[test]
  fn translate_sans_skips_greek() {
    // Arrange & Act
    let style = Some(MathStyle::Sans);
    assert_eq!(translate_math_char('a', style), '\u{1D5BA}'); // sans a
    assert_eq!(translate_math_char('1', style), '\u{1D7E3}'); // sans digit 1
    assert_eq!(translate_math_char('α', style), 'α', "Sans Greek は Unicode 未定義のため素通し");
  }

  #[test]
  fn translate_double_struck_handles_letterlike_holes() {
    // Arrange & Act
    let style = Some(MathStyle::DoubleStruck);
    assert_eq!(translate_math_char('A', style), '\u{1D538}'); // 連続 (穴なし)
    assert_eq!(translate_math_char('D', style), '\u{1D53B}'); // 連続
    assert_eq!(translate_math_char('C', style), '\u{2102}'); // ℂ (穴)
    assert_eq!(translate_math_char('H', style), '\u{210D}'); // ℍ (穴)
    assert_eq!(translate_math_char('N', style), '\u{2115}'); // ℕ (穴)
    assert_eq!(translate_math_char('R', style), '\u{211D}'); // ℝ (穴)
    assert_eq!(translate_math_char('Z', style), '\u{2124}'); // ℤ (穴)
    assert_eq!(translate_math_char('a', style), '\u{1D552}'); // 小文字は連続
    assert_eq!(translate_math_char('z', style), '\u{1D56B}');
    assert_eq!(translate_math_char('0', style), '\u{1D7D8}'); // 数字
    assert_eq!(translate_math_char('9', style), '\u{1D7E1}');
    assert_eq!(translate_math_char('α', style), 'α', "黒板太字 Greek は Unicode 未定義のため素通し");
  }

  #[test]
  fn translate_fraktur_handles_letterlike_holes() {
    // Arrange & Act
    let style = Some(MathStyle::Fraktur);
    assert_eq!(translate_math_char('A', style), '\u{1D504}'); // 連続
    assert_eq!(translate_math_char('C', style), '\u{212D}'); // ℭ (穴)
    assert_eq!(translate_math_char('H', style), '\u{210C}'); // ℌ (穴)
    assert_eq!(translate_math_char('I', style), '\u{2111}'); // ℑ (穴)
    assert_eq!(translate_math_char('R', style), '\u{211C}'); // ℜ (穴)
    assert_eq!(translate_math_char('Z', style), '\u{2128}'); // ℨ (穴)
    assert_eq!(translate_math_char('a', style), '\u{1D51E}'); // 小文字は連続
    assert_eq!(translate_math_char('z', style), '\u{1D537}');
    assert_eq!(translate_math_char('1', style), '1', "フラクトゥール数字は素通し");
  }

  #[test]
  fn translate_script_handles_letterlike_holes() {
    // Arrange & Act
    let style = Some(MathStyle::Script);
    assert_eq!(translate_math_char('A', style), '\u{1D49C}'); // 連続
    assert_eq!(translate_math_char('B', style), '\u{212C}'); // ℬ (穴)
    assert_eq!(translate_math_char('H', style), '\u{210B}'); // ℋ (穴)
    assert_eq!(translate_math_char('L', style), '\u{2112}'); // ℒ (穴)
    assert_eq!(translate_math_char('R', style), '\u{211B}'); // ℛ (穴)
    assert_eq!(translate_math_char('a', style), '\u{1D4B6}'); // 連続小文字
    assert_eq!(translate_math_char('e', style), '\u{212F}'); // ℯ (穴)
    assert_eq!(translate_math_char('g', style), '\u{210A}'); // ℊ (穴)
    assert_eq!(translate_math_char('o', style), '\u{2134}'); // ℴ (穴)
    assert_eq!(translate_math_char('1', style), '1', "スクリプト数字は素通し");
  }

  #[test]
  fn translate_script_bold_maps_contiguous_block() {
    // Arrange & Act
    let style = Some(MathStyle::ScriptBold);
    assert_eq!(translate_math_char('A', style), '\u{1D4D0}'); // 大文字 base 先頭
    assert_eq!(translate_math_char('Z', style), '\u{1D4E9}'); // 大文字 base 末尾
    assert_eq!(translate_math_char('a', style), '\u{1D4EA}'); // 小文字 base 先頭
    assert_eq!(translate_math_char('z', style), '\u{1D503}'); // 小文字 base 末尾
    assert_eq!(translate_math_char('1', style), '1', "太字スクリプト数字は素通し");
    assert_eq!(translate_math_char('α', style), 'α', "太字スクリプト Greek は Unicode 未定義のため素通し");
  }

  #[test]
  fn translate_calligraphic_reuses_script_codepoints() {
    // Arrange & Act
    let cal = Some(MathStyle::Calligraphic);
    let script = Some(MathStyle::Script);
    assert_eq!(translate_math_char('A', cal), translate_math_char('A', script));
    assert_eq!(translate_math_char('A', cal), '\u{1D49C}'); // 連続
    assert_eq!(translate_math_char('B', cal), '\u{212C}'); // ℬ (穴)
    assert_eq!(translate_math_char('e', cal), '\u{212F}'); // ℯ (穴)
    assert_eq!(translate_math_char('z', cal), '\u{1D4CF}'); // 小文字 base 末尾
    assert_eq!(translate_math_char('1', cal), '1', "カリグラフィー数字は素通し");
    assert_eq!(translate_math_char('α', cal), 'α', "カリグラフィー Greek は素通し");
  }

  #[test]
  fn push_math_char_appends_vs1_only_for_calligraphic_letters() {
    // Arrange
    let cal = Some(MathStyle::Calligraphic);

    // Act & Assert
    let mut a = String::new();
    push_math_char(&mut a, 'A', cal);
    assert_eq!(a, "\u{1D49C}\u{FE00}", "カリグラフィー英字は基底 + VS1");

    let mut hole = String::new();
    push_math_char(&mut hole, 'B', cal);
    assert_eq!(hole, "\u{212C}\u{FE00}", "Letterlike の穴文字にも VS1 を付与");

    let mut digit = String::new();
    push_math_char(&mut digit, '1', cal);
    assert_eq!(digit, "1", "数字には VS1 を付けない");
  }

  #[test]
  fn push_math_char_keeps_other_styles_unchanged() {
    // Arrange & Act
    let mut script = String::new();
    push_math_char(&mut script, 'A', Some(MathStyle::Script));
    assert_eq!(script, "\u{1D49C}", "スクリプトは VS1 無し（既存挙動不変）");

    let mut default = String::new();
    push_math_char(&mut default, 'x', None);
    assert_eq!(default, "\u{1D465}", "デフォルトは italic 化のみ");
  }

  #[test]
  fn translate_fraktur_bold_maps_contiguous_block() {
    // Arrange & Act
    let style = Some(MathStyle::FrakturBold);
    assert_eq!(translate_math_char('A', style), '\u{1D56C}'); // 大文字 base 先頭
    assert_eq!(translate_math_char('Z', style), '\u{1D585}'); // 大文字 base 末尾
    assert_eq!(translate_math_char('a', style), '\u{1D586}'); // 小文字 base 先頭
    assert_eq!(translate_math_char('z', style), '\u{1D59F}'); // 小文字 base 末尾
    assert_eq!(translate_math_char('1', style), '1', "太字フラクトゥール数字は素通し");
    assert_eq!(translate_math_char('α', style), 'α', "太字フラクトゥール Greek は Unicode 未定義のため素通し");
  }
}
