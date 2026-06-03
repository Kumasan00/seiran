//! 脚注（footnote）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 脚注のスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct FootnoteStyle {
  /// 脚注テキストのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  /// 脚注テキストの行高係数
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 本文と脚注を区切る罫線の幅（pt）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub separator_width: f32,
  /// 区切り罫線の太さ（pt）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub separator_thickness: f32,
  /// 本文末と脚注領域の間の上余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub top_margin: f32,
  /// 脚注マーカーの書式テンプレート。`{number}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub marker_format: String,
}

impl Default for FootnoteStyle {
  fn default() -> Self {
    return Self {
      font_size: 9.0,
      line_height_factor: 1.1,
      separator_width: 80.0,
      separator_thickness: 0.5,
      top_margin: 8.0,
      marker_format: "{number}".to_string(),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::FootnoteStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(FootnoteStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = FootnoteStyle {
      font_size: 0.0,
      ..FootnoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    // Arrange
    let style = FootnoteStyle {
      line_height_factor: 0.0,
      ..FootnoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_separator_width() {
    // Arrange: 区切り線非表示相当
    let style = FootnoteStyle {
      separator_width: 0.0,
      ..FootnoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_marker_format() {
    // Arrange
    let style = FootnoteStyle {
      marker_format: String::new(),
      ..FootnoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    // Arrange
    let style = FootnoteStyle {
      top_margin: -1.0,
      ..FootnoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
