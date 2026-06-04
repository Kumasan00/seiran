//! 図（figure）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 図環境のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct FigureStyle {
  /// キャプションの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub caption_format: String,
  /// キャプションを図本体の上下どちらに配置するか
  pub caption_position: CaptionPosition,
  /// キャプションのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub caption_font_size: f32,
  /// 図ブロックの上余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub top_margin: f32,
  /// 図ブロックの下余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  /// 図本体とキャプションの間隔
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub inner_margin: f32,
}

impl Default for FigureStyle {
  fn default() -> Self {
    return Self {
      caption_format: "Figure {number}: {title}".to_string(),
      caption_position: CaptionPosition::Bottom,
      caption_font_size: 11.0,
      top_margin: 12.0,
      bottom_margin: 12.0,
      inner_margin: 6.0,
    };
  }
}

/// キャプション位置（図・表で共用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum CaptionPosition {
  /// キャプションを本体の上に配置
  Top,
  /// キャプションを本体の下に配置
  Bottom,
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{CaptionPosition, FigureStyle};

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
    let style = FigureStyle {
      caption_format: String::new(),
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_caption_font_size() {
    let style = FigureStyle {
      caption_font_size: 0.0,
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    let style = FigureStyle {
      top_margin: -1.0,
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
