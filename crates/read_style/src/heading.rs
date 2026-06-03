//! 見出し要素（part / chapter / section …）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 見出し要素のスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct HeadingStyle {
  /// 見出しの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub format: String,
  /// 見出しテキストのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  /// 見出しブロックの下余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  /// 見出しの直前で改ページするか
  pub page_break_before: bool,
  /// 見出しの直後で改ページするか
  pub page_break_after: bool,
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::HeadingStyle;

  /// 妥当な [`HeadingStyle`] を返すテストヘルパー
  fn valid_heading() -> HeadingStyle {
    return HeadingStyle {
      format: "Chapter {number}".to_string(),
      font_size: 12.0,
      bottom_margin: 10.0,
      page_break_before: false,
      page_break_after: false,
    };
  }

  #[test]
  fn validate_accepts_valid_values() {
    // Arrange
    let heading = valid_heading();

    // Act / Assert
    assert!(heading.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    // Arrange
    let mut heading = valid_heading();
    heading.format = String::new();

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange: range(min = f32::MIN_POSITIVE) のため 0.0 は不正
    let mut heading = valid_heading();
    heading.font_size = 0.0;

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_font_size() {
    // Arrange
    let mut heading = valid_heading();
    heading.font_size = -1.0;

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_bottom_margin() {
    // Arrange: bottom_margin は range(min = 0.0) なので 0.0 は許容される
    let mut heading = valid_heading();
    heading.bottom_margin = 0.0;

    // Act / Assert
    assert!(heading.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_bottom_margin() {
    // Arrange
    let mut heading = valid_heading();
    heading.bottom_margin = -0.1;

    // Act / Assert
    assert!(heading.validate().is_err());
  }
}
