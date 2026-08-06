//! 8bit RGB 色 [`Color`]。

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

  /// `"#rrggbb"` 形式の文字列を `Color` に変換する
  ///
  /// 大文字小文字は区別しない。不正な形式には `None` を返す。
  #[must_use]
  pub fn from_hex(value: &str) -> Option<Color> {
    let body = value.strip_prefix('#')?;
    if body.len() != 6 || !body.chars().all(|c| return c.is_ascii_hexdigit()) {
      return None;
    }
    let r = u8::from_str_radix(&body[0..2], 16).ok()?;
    let g = u8::from_str_radix(&body[2..4], 16).ok()?;
    let b = u8::from_str_radix(&body[4..6], 16).ok()?;
    return Some(Color([r, g, b]));
  }
}

impl From<[u8; 3]> for Color {
  fn from(rgb: [u8; 3]) -> Self { return Color(rgb); }
}

impl From<Color> for [u8; 3] {
  fn from(color: Color) -> Self { return color.0; }
}

impl<'de> Deserialize<'de> for Color {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let hex = String::deserialize(deserializer)?;
    return Color::from_hex(&hex)
      .ok_or_else(|| return D::Error::custom(format!("色の 16 進表記が不正です: {hex:?}（期待形式: \"#rrggbb\"）")));
  }
}

impl Serialize for Color {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let [r, g, b] = self.0;
    return format!("#{r:02x}{g:02x}{b:02x}").serialize(serializer);
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
    // Arrange / Act
    let result: Result<Wrapper, _> = toml::from_str("color = [204, 179, 153]");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn parses_hex_lowercase() {
    // Arrange / Act
    let w: Wrapper = toml::from_str("color = \"#cc9966\"").unwrap();

    // Assert
    assert_eq!(w.color, Color::new(0xcc, 0x99, 0x66));
  }

  #[test]
  fn parses_hex_uppercase() {
    // Arrange / Act
    let w: Wrapper = toml::from_str("color = \"#CC9966\"").unwrap();

    // Assert
    assert_eq!(w.color, Color::new(0xcc, 0x99, 0x66));
  }

  #[test]
  fn rejects_invalid_hex_length() {
    // Arrange / Act
    let result: Result<Wrapper, _> = toml::from_str("color = \"#abcde\"");

    // Assert
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
    // Arrange / Act
    let result: Result<Wrapper, _> = toml::from_str("color = \"#gghhii\"");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn from_hex_parses_valid_string() {
    // Arrange / Act
    let color = Color::from_hex("#FF0000");

    // Assert
    assert_eq!(color, Some(Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn from_hex_rejects_invalid_string() {
    // Arrange / Act / Assert
    assert_eq!(Color::from_hex("ff0000"), None);
    assert_eq!(Color::from_hex("#fff"), None);
    assert_eq!(Color::from_hex("#gg0000"), None);
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
