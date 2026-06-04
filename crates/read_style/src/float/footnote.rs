//! 脚注（footnote）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 脚注のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
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
    assert!(FootnoteStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = FootnoteStyle {
      font_size: 0.0,
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_marker_format() {
    let style = FootnoteStyle {
      marker_format: String::new(),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
