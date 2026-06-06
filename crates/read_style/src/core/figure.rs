//! 図（figure）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::length::{Length, non_negative};

use crate::core::caption::{CaptionPosition, CaptionStyle};

/// 図環境のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct FigureStyle {
  /// キャプション本体（書式テンプレートとフォントサイズ）
  #[garde(dive)]
  pub caption: CaptionStyle,
  /// キャプションを図本体の上下どちらに配置するか
  pub caption_position: CaptionPosition,
  /// 図ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 図ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 図本体とキャプションの間隔
  #[garde(custom(non_negative))]
  pub inner_margin: Length,
}

impl Default for FigureStyle {
  fn default() -> Self {
    return Self {
      caption: CaptionStyle {
        format: "Figure {number}: {title}".to_string(),
        font_size: Length::pt(11.0),
      },
      caption_position: CaptionPosition::Bottom,
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::length::Length;

  use super::FigureStyle;
  use crate::core::caption::CaptionPosition;

  #[test]
  fn validate_accepts_default() {
    assert!(FigureStyle::default().validate().is_ok());
  }

  #[test]
  fn default_has_bottom_caption() {
    assert_eq!(FigureStyle::default().caption_position, CaptionPosition::Bottom);
  }

  #[test]
  fn validate_rejects_empty_caption_format() {
    let mut style = FigureStyle::default();
    style.caption.format = String::new();
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_caption_font_size() {
    let mut style = FigureStyle::default();
    style.caption.font_size = Length::pt(0.0);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    let style = FigureStyle {
      top_margin: Length::pt(-1.0),
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
