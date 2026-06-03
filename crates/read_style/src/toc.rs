//! 目次（table of contents）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 目次のスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct TocStyle {
  /// 目次のタイトル文字列
  #[garde(length(chars, min = 1))]
  pub title: String,
  /// 目次に含める見出しの最大深さ。1=part のみ、6=subparagraph まで
  #[garde(range(min = 1, max = 6))]
  pub max_depth: u32,
  /// 目次エントリのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  /// 目次ブロックの下余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  /// ページ番号を表示するか
  pub show_page_numbers: bool,
}

impl Default for TocStyle {
  fn default() -> Self {
    return Self {
      title: "Contents".to_string(),
      max_depth: 3,
      font_size: 12.0,
      bottom_margin: 10.0,
      show_page_numbers: true,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TocStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(TocStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_title() {
    // Arrange
    let style = TocStyle {
      title: String::new(),
      ..TocStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_max_depth() {
    // Arrange
    let style = TocStyle {
      max_depth: 0,
      ..TocStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_too_large_max_depth() {
    // Arrange: 見出しは subparagraph まで 6 レベル
    let style = TocStyle {
      max_depth: 7,
      ..TocStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = TocStyle {
      font_size: 0.0,
      ..TocStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_bottom_margin() {
    // Arrange
    let style = TocStyle {
      bottom_margin: -1.0,
      ..TocStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
