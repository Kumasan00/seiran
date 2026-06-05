//! 表（table）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  Color,
  core::caption::{CaptionPosition, CaptionStyle},
  primitives::length::{Length, non_negative},
};

/// 表のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TableStyle {
  /// キャプション本体（書式テンプレートとフォントサイズ）
  #[garde(dive)]
  pub caption: CaptionStyle,
  /// キャプションを表本体の上下どちらに配置するか
  pub caption_position: CaptionPosition,
  /// 表ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 表ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 罫線の太さ
  #[garde(custom(non_negative))]
  pub rule_thickness: Length,
  /// 罫線色。`None` は黒
  pub rule_color: Option<Color>,
}

impl Default for TableStyle {
  fn default() -> Self {
    return Self {
      caption: CaptionStyle {
        format: "Table {number}: {title}".to_string(),
        font_size: Length::pt(11.0),
      },
      caption_position: CaptionPosition::Top,
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
      rule_thickness: Length::pt(0.5),
      rule_color: None,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TableStyle;
  use crate::{core::caption::CaptionPosition, primitives::length::Length};

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
      rule_thickness: Length::pt(-0.1),
      ..TableStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
