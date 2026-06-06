//! 参考文献セクションのスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::primitives::length::{Length, non_negative, positive};

/// 参考文献セクションのスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct ReferenceStyle {
  /// 参考文献セクションのタイトル文字列
  #[garde(length(chars, min = 1))]
  pub title: String,
  /// セクションのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// セクションブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      title: "References".to_string(),
      font_size: Length::pt(12.0),
      bottom_margin: Length::pt(10.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::ReferenceStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(ReferenceStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    let style = ReferenceStyle {
      title: String::new(),
      ..ReferenceStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
