//! 色を表す `Color` newtype。
//!
//! TOML 上では以下の 2 形式を受け付ける:
//!
//! - 配列形式: `text_color = [204, 179, 153]`
//! - 16 進文字列形式: `text_color = "#cc9966"` または `"#CC9966"`
//!
//! いずれも 8bit RGB に正規化して `Color([u8; 3])` に格納する。
//! u8 範囲外の値は serde 段階で拒否される。

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

/// 8bit RGB 色（`[u8; 3]`）の newtype
///
/// PDF レンダリング側に渡す前段の入口で色表現を統一する。
/// 透明度（alpha）が必要になった場合はバリアントを増やすか別型を作る。
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

/// `"#rrggbb"` 形式の文字列を 3 バイトの配列に変換する
fn parse_hex(value: &str) -> Option<[u8; 3]> {
  let body = value.strip_prefix('#')?;
  if body.len() != 6 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
    return None;
  }
  let r = u8::from_str_radix(&body[0..2], 16).ok()?;
  let g = u8::from_str_radix(&body[2..4], 16).ok()?;
  let b = u8::from_str_radix(&body[4..6], 16).ok()?;
  return Some([r, g, b]);
}

impl<'de> Deserialize<'de> for Color {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    /// 配列・文字列の両方を受ける中間表現
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
      Hex(String),
      Rgb([u8; 3]),
    }
    let repr = Repr::deserialize(deserializer)?;
    return match repr {
      Repr::Rgb(rgb) => Ok(Color(rgb)),
      Repr::Hex(s) => parse_hex(&s)
        .map(Color)
        .ok_or_else(|| D::Error::custom(format!("色の 16 進表記が不正です: {s:?}（期待形式: \"#rrggbb\"）"))),
    };
  }
}

impl Serialize for Color {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // 配列形式で書き出す（往復性を最優先）
    return self.0.serialize(serializer);
  }
}

#[cfg(test)]
mod tests {
  use serde::Deserialize;

  use super::Color;

  #[derive(Debug, Deserialize)]
  struct Wrapper {
    color: Color,
  }

  #[test]
  fn parses_rgb_array() {
    // Arrange / Act
    let w: Wrapper = toml::from_str("color = [204, 179, 153]").unwrap();

    // Assert
    assert_eq!(w.color, Color::new(204, 179, 153));
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
    // Arrange: 5 文字
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
  fn rejects_out_of_range_value() {
    // Arrange: R が 256
    let result: Result<Wrapper, _> = toml::from_str("color = [256, 0, 0]");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn rejects_negative_value() {
    // Arrange
    let result: Result<Wrapper, _> = toml::from_str("color = [-1, 0, 0]");

    // Assert
    assert!(result.is_err());
  }
}
