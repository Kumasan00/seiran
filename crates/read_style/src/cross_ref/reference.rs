//! 参考文献セクションのスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 参考文献セクションのスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct ReferenceStyle {
  /// 参考文献セクションのタイトル文字列
  #[garde(length(chars, min = 1))]
  pub format: String,
  /// セクションのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  /// セクションブロックの下余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      format: "References".to_string(),
      font_size: 12.0,
      bottom_margin: 10.0,
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
      format: String::new(),
      ..ReferenceStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
