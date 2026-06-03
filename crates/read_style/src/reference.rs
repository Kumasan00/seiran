//! 参考文献セクションのスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 参考文献セクションのスタイル設定（フォーマット文字列・フォントサイズ・下余白）
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct ReferenceStyle {
  #[garde(length(chars, min = 1))]
  pub format: String,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      format: "References".to_string(),
      font_size: 12.0,
      bottom_margin: 10.0,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::ReferenceStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(ReferenceStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    // Arrange
    let style = ReferenceStyle {
      format: String::new(),
      ..ReferenceStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = ReferenceStyle {
      font_size: 0.0,
      ..ReferenceStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_font_size() {
    // Arrange
    let style = ReferenceStyle {
      font_size: -1.0,
      ..ReferenceStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_bottom_margin() {
    // Arrange: bottom_margin は range(min = 0.0) のため 0.0 は許容される
    let style = ReferenceStyle {
      bottom_margin: 0.0,
      ..ReferenceStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_bottom_margin() {
    // Arrange
    let style = ReferenceStyle {
      bottom_margin: -1.0,
      ..ReferenceStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
