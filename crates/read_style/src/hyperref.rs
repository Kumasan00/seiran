//! ハイパーリンク（hyperref 相当）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// ハイパーリンクとしおりに関するスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct HyperrefStyle {
  /// 内部参照リンク（`\ref` 等）の文字色 RGB。`None` は本文色を継承
  pub link_color: Option<[u8; 3]>,
  /// 外部 URL リンクの文字色 RGB。`None` は本文色を継承
  pub url_color: Option<[u8; 3]>,
  /// 文献引用（`\cite`）の文字色 RGB。`None` は本文色を継承
  pub cite_color: Option<[u8; 3]>,
  /// PDF のしおり（ブックマーク）を出力するか
  pub show_bookmarks: bool,
}

impl Default for HyperrefStyle {
  fn default() -> Self {
    return Self {
      link_color: Some([0, 0, 255]),
      url_color: Some([0, 0, 255]),
      cite_color: Some([0, 0, 255]),
      show_bookmarks: true,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::HyperrefStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(HyperrefStyle::default().validate().is_ok());
  }

  #[test]
  fn default_enables_bookmarks() {
    // Arrange / Act
    let style = HyperrefStyle::default();

    // Assert
    assert!(style.show_bookmarks);
  }

  #[test]
  fn validate_accepts_all_colors_none() {
    // Arrange
    let style = HyperrefStyle {
      link_color: None,
      url_color: None,
      cite_color: None,
      show_bookmarks: false,
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
