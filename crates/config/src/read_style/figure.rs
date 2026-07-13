//! 図（figure）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{Length, length::non_negative};

use crate::read_style::caption::CaptionStyle;

/// 図環境のスタイル設定
///
/// キャプション位置は `\image` と `\caption` のソース上の出現順で決まるため、
/// スタイル側では持たない。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct FigureStyle {
  /// キャプション本体（書式テンプレートとフォントサイズ）
  #[garde(dive)]
  pub caption: CaptionStyle,
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
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::Length;

  use super::FigureStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(FigureStyle::default().validate().is_ok());
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

  #[test]
  fn rejects_moved_image_keys() {
    // `max_dpi` / `downsample` は出力物理の設定として config.toml `[image]` へ移動した（#125）。
    // style 側旧キーは `deny_unknown_fields` で未知フィールドとして弾く。
    assert!(toml::from_str::<FigureStyle>("max_dpi = 300\n").is_err());
    assert!(toml::from_str::<FigureStyle>("downsample = true\n").is_err());
  }
}
