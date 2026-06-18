//! 図（figure）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::length::{Length, non_negative};

use crate::caption::CaptionStyle;

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
  /// ラスタ画像埋め込み時の最大 DPI。`\image[dpi=...]` で per-image 上書き可能
  ///
  /// 表示物理サイズと本値から必要ピクセル数を計算し、元画像がそれを超える場合に限り縮小する。
  /// `downsample` が `false` の場合や `\image[downsample=false]` が指定された場合は無視される。
  #[garde(range(min = 1, max = 2400))]
  pub max_dpi: u32,
  /// ラスタ画像のダウンサンプリングを行うかどうか
  ///
  /// `false` の場合、`max_dpi` の値にかかわらず全画像を原寸で埋め込む。
  /// `\image[downsample=false]` で per-image 上書きが可能。
  #[garde(skip)]
  pub downsample: bool,
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
      max_dpi: 300,
      downsample: true,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::length::Length;

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
  fn validate_rejects_zero_max_dpi() {
    let style = FigureStyle {
      max_dpi: 0,
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_huge_max_dpi() {
    let style = FigureStyle {
      max_dpi: 9999,
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
