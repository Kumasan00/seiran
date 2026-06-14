//! 色を表す `Color` newtype。
//!
//! TOML / コマンド任意引数のいずれでも `"#rrggbb"` の 16 進文字列形式のみを受け付ける
//! （大文字小文字は不問）。`[204, 179, 153]` の配列形式は受け付けない
//! （破壊的: スタイル側の視認性を優先）。
//!
//! 内部表現は 8bit RGB に正規化した `Color([u8; 3])`。背景色・罫線色・テキスト色など
//! 全クレート共通の色表現としてここ（`types`）に置く。

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

  /// `"#rrggbb"` 形式の文字列を `Color` に変換する（大文字小文字不問）
  ///
  /// `#` 接頭辞・6 桁・全桁が 16 進数のいずれかを満たさない場合は `None` を返す。
  /// デシリアライズとパーサの任意引数解析の双方から再利用する。
  #[must_use]
  pub fn from_hex(value: &str) -> Option<Color> {
    let body = value.strip_prefix('#')?;
    if body.len() != 6 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
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
      .ok_or_else(|| D::Error::custom(format!("色の 16 進表記が不正です: {hex:?}（期待形式: \"#rrggbb\"）")));
  }
}

impl Serialize for Color {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // 内部表現は RGB だが、デシリアライズと往復させるため `#rrggbb` 形式で書き出す
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
    // Arrange: 旧形式の配列指定は受け付けない（破壊的）
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
  fn rejects_non_hex_chars_in_body() {
    // Arrange: 'g' は 16 進数ではない
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
    // Arrange / Act / Assert — 接頭辞なし・桁数不足・非 16 進はいずれも None
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

    // Assert: 内部表現と往復可能な形式で書き出される
    assert!(s.contains("color = \"#cc9966\""), "serialized form: {s}");
  }
}
