//! 図・表のキャプションスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  length::{Length, positive},
  style::NumberTitleTemplate,
};

/// キャプションの共通設定。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct CaptionStyle {
  /// キャプションの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(dive)]
  pub format: NumberTitleTemplate,
  /// キャプションのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
}

impl Default for CaptionStyle {
  fn default() -> Self {
    return Self {
      format: NumberTitleTemplate::parse("{number}: {title}"),
      font_size: Length::pt(11.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::CaptionStyle;
  use crate::{length::Length, style::NumberTitleTemplate};

  #[test]
  fn validate_rejects_empty_format() {
    let style = CaptionStyle {
      format: NumberTitleTemplate::parse(""),
      ..CaptionStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = CaptionStyle {
      font_size: Length::pt(0.0),
      ..CaptionStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn deserializes_partial_table_with_default_font_size() {
    // Arrange
    let toml = "format = \"Figure {number}: {title}\"\n";

    // Act
    let style: CaptionStyle = toml::from_str(toml).unwrap();

    // Assert
    assert_eq!(style.format.as_str(), "Figure {number}: {title}");
    assert!((style.font_size.to_pt() - 11.0).abs() < f32::EPSILON);
  }
}
