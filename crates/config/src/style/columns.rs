//! 段組み（1 段 / 2 段切替）のスタイル設定型。
//!
//! 段組み数は「同じ本文 + 同じ用紙で見た目だけ差し替える」対象なので `style.toml`（`[columns]`）に置く。
//! 用紙サイズ・余白は物理設定として `config.toml` 側に残る。段組みのフロー（左段 → 右段 → 次ページ）は
//! `typeset::breaking::break_pages` が担い、本型はその段数と段間だけを保持する。

use garde::Validate;
use model::{Length, length::non_negative};
use serde::{Deserialize, Serialize};

/// 段組みのスタイル設定
///
/// `count` は段数（既定 1 = 単段）。issue #32 のスコープに合わせ 1〜2 段に限定する
/// （`typeset::breaking` のフロー自体は任意 N に一般化されるが、契約としては `[1, 2]` を受け入れる）。
/// `gap` は段間（gutter、既定 18pt）。本文幅 `text_width` から段幅は
/// `(text_width - (count - 1) * gap) / count` で求める（[`model::column_width`]）。
/// `count` と `config.toml` の用紙・余白との横断制約は [`crate::validate_layout`] が検証する。
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
  use model::Length;

  use super::ColumnsStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(ColumnsStyle::default().validate().is_ok());
  }

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
    // Arrange — issue #32 のスコープは 1〜2 段。3 段は契約外
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
