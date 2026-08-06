//! ページ組版の挙動スタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// ページ組版の挙動スタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct PageStyle {
  /// 下端揃えを有効にするか
  pub flush_bottom: bool,
}

impl Default for PageStyle {
  fn default() -> Self {
    return Self {
      flush_bottom: false,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::PageStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(PageStyle::default().validate().is_ok());
  }

  #[test]
  fn default_disables_flush_bottom() {
    // Arrange / Act
    let style = PageStyle::default();

    // Assert
    assert!(!style.flush_bottom);
  }

  #[test]
  fn validate_accepts_enabled_flush_bottom() {
    // Arrange
    let style = PageStyle { flush_bottom: true };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
