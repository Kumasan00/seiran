//! 表（table）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::Color;
pub use crate::float::figure::CaptionPosition;

/// 表のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TableStyle {
  /// キャプションの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub caption_format: String,
  /// キャプションを表本体の上下どちらに配置するか
  pub caption_position: CaptionPosition,
  /// キャプションのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub caption_font_size: f32,
  /// 表ブロックの上余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub top_margin: f32,
  /// 表ブロックの下余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  /// 罫線の太さ（pt）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub rule_thickness: f32,
  /// 罫線色。`None` は黒
  pub rule_color: Option<Color>,
}

impl Default for TableStyle {
  fn default() -> Self {
    return Self {
      caption_format: "Table {number}: {title}".to_string(),
      caption_position: CaptionPosition::Top,
      caption_font_size: 11.0,
      top_margin: 12.0,
      bottom_margin: 12.0,
      rule_thickness: 0.5,
      rule_color: None,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{CaptionPosition, TableStyle};

  #[test]
  fn validate_accepts_default() {
    assert!(TableStyle::default().validate().is_ok());
  }

  #[test]
  fn default_has_top_caption() {
    assert_eq!(TableStyle::default().caption_position, CaptionPosition::Top);
  }

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    let style = TableStyle {
      rule_thickness: -0.1,
      ..TableStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
