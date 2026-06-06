//! 脚注（footnote）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::length::{Length, non_negative, positive};

/// 脚注のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct FootnoteStyle {
  /// 脚注テキストのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 脚注テキストの行高係数
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 本文と脚注を区切る罫線の幅
  #[garde(custom(non_negative))]
  pub separator_width: Length,
  /// 区切り罫線の太さ
  #[garde(custom(non_negative))]
  pub separator_thickness: Length,
  /// 本文末と脚注領域の間の上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 脚注マーカーの書式テンプレート。`{number}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub marker_format: String,
}

impl Default for FootnoteStyle {
  fn default() -> Self {
    return Self {
      font_size: Length::pt(9.0),
      line_height_factor: 1.1,
      separator_width: Length::pt(80.0),
      separator_thickness: Length::pt(0.5),
      top_margin: Length::pt(8.0),
      marker_format: "{number}".to_string(),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::length::Length;

  use super::FootnoteStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(FootnoteStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = FootnoteStyle {
      font_size: Length::pt(0.0),
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
