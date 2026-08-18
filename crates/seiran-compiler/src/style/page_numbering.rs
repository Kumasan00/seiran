//! ページ番号のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::style::number_style::NumberStyle;

/// ページ番号のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct PageNumbering {
  /// 前付け（タイトルページ・目次）ページの番号表記。既定は小文字ローマ数字（i, ii, …）
  pub front_matter: NumberStyle,
  /// 本文ページの番号表記。既定は算用数字（1, 2, …）。本文は常に 1 から振り直す
  pub body: NumberStyle,
}

impl Default for PageNumbering {
  fn default() -> Self {
    return Self {
      front_matter: NumberStyle::RomanLower,
      body: NumberStyle::Arabic,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::PageNumbering;
  use crate::style::number_style::NumberStyle;

  #[test]
  fn default_uses_roman_front_and_arabic_body() {
    let pn = PageNumbering::default();
    assert_eq!(pn.front_matter, NumberStyle::RomanLower);
    assert_eq!(pn.body, NumberStyle::Arabic);
  }

  #[test]
  fn default_round_trips_through_toml() {
    // Arrange / Act
    let pn = PageNumbering::default();
    let toml = toml::to_string(&pn).unwrap();
    let restored: PageNumbering = toml::from_str(&toml).unwrap();

    // Assert
    assert_eq!(restored.front_matter, pn.front_matter);
    assert_eq!(restored.body, pn.body);
  }
}
