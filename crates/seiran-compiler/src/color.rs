//! 8bit RGB 色 [`Color`]。
//!
//! TOML 上では `"#rrggbb"` の 16 進文字列で指定する（`[r, g, b]` 配列は受け付けない）。
//! 文字列との相互変換は [`FromStr`] / [`Display`](fmt::Display) の正準形 `#rrggbb`（小文字）に
//! 集約し、serde の [`Serialize`] / [`Deserialize`] もこの 2 実装へ委譲する。

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

/// 8bit RGB 色（`[u8; 3]`）の newtype
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub [u8; 3]);

impl Color {
  /// 新しい `Color` を作る
  #[must_use]
  pub fn new(r: u8, g: u8, b: u8) -> Self { return Color([r, g, b]); }

  /// R / G / B 各成分を返す
  #[must_use]
  pub fn rgb(self) -> [u8; 3] { return self.0; }
}

impl From<[u8; 3]> for Color {
  fn from(rgb: [u8; 3]) -> Self { return Color(rgb); }
}

impl From<Color> for [u8; 3] {
  fn from(color: Color) -> Self { return color.0; }
}

/// [`Color`] の文字列パース失敗を表すエラー。
///
/// `#rrggbb` 以外の形式で [`Color::from_str`] が返す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseColorError {
  /// パースに失敗した入力文字列。
  input: String,
}

impl fmt::Display for ParseColorError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return write!(f, "色は `#rrggbb` 形式の 16 進表記で指定してください: {:?}", self.input);
  }
}

impl std::error::Error for ParseColorError {}

impl FromStr for Color {
  type Err = ParseColorError;

  /// `"#rrggbb"` 形式の文字列を `Color` に変換する。大文字小文字は区別しない。
  ///
  /// [`Length`](crate::Length) と違い前後の空白は許容しない（style.toml の受理範囲を
  /// `"#rrggbb"` ちょうどに保つため）。空白を落とす必要がある呼び出し側は自分で `trim` する。
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let invalid = || {
      return ParseColorError {
        input: s.to_string(),
      };
    };
    let body = s.strip_prefix('#').ok_or_else(invalid)?;
    if body.len() != 6 || !body.chars().all(|c| return c.is_ascii_hexdigit()) {
      return Err(invalid());
    }
    // 本体は ASCII 6 桁と確定しているので、2 桁ずつのバイト添字は char 境界を割らない。
    let component = |digits: &str| {
      let Ok(value) = u8::from_str_radix(digits, 16) else {
        unreachable!("ASCII 16 進数字 2 桁は u8 に収まる（直前の桁数・字種検査が保証している）");
      };
      return value;
    };
    return Ok(Color([
      component(&body[0..2]),
      component(&body[2..4]),
      component(&body[4..6]),
    ]));
  }
}

impl fmt::Display for Color {
  /// 正準の人間可読表現 `#rrggbb`（小文字）。[`Color::from_str`] と往復する。
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let [r, g, b] = self.0;
    return write!(f, "#{r:02x}{g:02x}{b:02x}");
  }
}

impl<'de> Deserialize<'de> for Color {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    return value.parse().map_err(D::Error::custom);
  }
}

impl Serialize for Color {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // 正準形は [`Display`](fmt::Display) が唯一の定義箇所。`Length` と違い `#rrggbb` は
    // 無損失なので、往復精度のための別実装を持たない。
    return serializer.serialize_str(&self.to_string());
  }
}

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Serialize};

  use super::Color;

  #[derive(Debug, Deserialize)]
  struct Wrapper {
    color: Color,
  }

  #[derive(Debug, Serialize)]
  struct SerWrapper {
    color: Color,
  }

  #[test]
  fn rejects_rgb_array() {
    let result: Result<Wrapper, _> = toml::from_str("color = [204, 179, 153]");

    assert!(result.is_err());
  }

  #[test]
  fn parses_hex_lowercase() {
    let w: Wrapper = toml::from_str("color = \"#cc9966\"").unwrap();

    assert_eq!(w.color, Color::new(0xcc, 0x99, 0x66));
  }

  #[test]
  fn parses_hex_uppercase() {
    let w: Wrapper = toml::from_str("color = \"#CC9966\"").unwrap();

    assert_eq!(w.color, Color::new(0xcc, 0x99, 0x66));
  }

  #[test]
  fn rejects_invalid_hex_length() {
    let result: Result<Wrapper, _> = toml::from_str("color = \"#abcde\"");

    assert!(result.is_err());
  }

  #[test]
  fn rejects_hex_without_prefix() {
    // Arrange
    let result: Result<Wrapper, _> = toml::from_str("color = \"cc9966\"");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn rejects_non_hex_chars_in_body() {
    let result: Result<Wrapper, _> = toml::from_str("color = \"#gghhii\"");

    assert!(result.is_err());
  }

  #[test]
  fn from_str_parses_valid_string() {
    let color = "#FF0000".parse::<Color>();

    assert_eq!(color, Ok(Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn from_str_rejects_invalid_string() {
    assert!("ff0000".parse::<Color>().is_err());
    assert!("#fff".parse::<Color>().is_err());
    assert!("#gg0000".parse::<Color>().is_err());
  }

  #[test]
  fn from_str_rejects_surrounding_whitespace() {
    // 前後空白は呼び出し側の責務（受理範囲を `#rrggbb` ちょうどに保つ）
    assert!(" #cc9966".parse::<Color>().is_err());
    assert!("#cc9966 ".parse::<Color>().is_err());
  }

  #[test]
  fn display_and_from_str_round_trip() {
    // Arrange
    let value = Color::new(0xcc, 0x99, 0x66);

    // Act: Display の正準形 `#rrggbb` を FromStr で往復
    let text = value.to_string();

    // Assert
    assert_eq!(text, "#cc9966");
    assert_eq!(text.parse::<Color>().unwrap(), value);
  }

  #[test]
  fn serializes_to_lowercase_hex_string() {
    // Arrange
    let value = SerWrapper {
      color: Color::new(0xcc, 0x99, 0x66),
    };

    // Act
    let s = toml::to_string(&value).unwrap();

    // Assert
    assert!(s.contains("color = \"#cc9966\""), "serialized form: {s}");
  }
}
