//! 段組み（1 段 / 2 段切替）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::length::{Length, non_negative};

/// 段組みのスタイル設定
///
/// `count` と `config.toml` の用紙・余白との横断制約は [`crate::typeset::validate_layout`] が検証する。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct ColumnsStyle {
  /// 段数（1 = 単段、2 = 2 段組）
  #[garde(range(min = 1, max = 2))]
  pub count: u8,
  /// 段間（gutter）
  #[garde(custom(non_negative))]
  pub gap: Length,
}

impl Default for ColumnsStyle {
  fn default() -> Self {
    return Self {
      count: 1,
      gap: Length::pt(18.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::ColumnsStyle;
  use crate::length::Length;

  #[test]
  fn default_matches_documented_values() {
    // Arrange / Act
    let style = ColumnsStyle::default();

    // Assert
    assert_eq!(style.count, 1);
    assert!((style.gap.to_pt() - 18.0).abs() < f32::EPSILON);
  }

  #[test]
  fn validate_accepts_two_columns() {
    // Arrange
    let style = ColumnsStyle {
      count: 2,
      ..ColumnsStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_count() {
    // Arrange
    let style = ColumnsStyle {
      count: 0,
      ..ColumnsStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_three_columns() {
    // Arrange
    let style = ColumnsStyle {
      count: 3,
      ..ColumnsStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_gap() {
    // Arrange
    let style = ColumnsStyle {
      gap: Length::pt(0.0),
      ..ColumnsStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_gap() {
    // Arrange
    let style = ColumnsStyle {
      gap: Length::pt(-1.0),
      ..ColumnsStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
