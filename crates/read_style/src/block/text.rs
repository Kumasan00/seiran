//! 本文段落（`DocNode::Paragraph`）のスタイル設定型。
//!
//! 見出しレベルの `Paragraph` と区別するため、本文段落は `TextBlockStyle` という名前にし、
//! `Style::text` フィールドに置く。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::FontKind;

use crate::common::length::{Length, non_negative};

/// 本文段落のスタイル設定
///
/// `paragraph_spacing` は段落末に挿入するスペース。
/// 旧 `inter_paragraph_spacing: Option<f32>` の `None`（未指定 → `font_size`）挙動を廃止し、
/// 明示的な既定値（12.0pt）を使う。サイズを動的に追従させたければ呼び出し側で計算する。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TextBlockStyle {
  /// 段落末に挿入するスペース
  #[garde(custom(non_negative))]
  pub paragraph_spacing: Length,
  /// 段落本文のフォント種別
  pub font_kind: FontKind,
}

impl Default for TextBlockStyle {
  fn default() -> Self {
    return Self {
      paragraph_spacing: Length::pt(12.0),
      font_kind: FontKind::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::FontKind;

  use super::TextBlockStyle;
  use crate::common::length::Length;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(TextBlockStyle::default().validate().is_ok());
  }

  #[test]
  fn default_matches_documented_values() {
    // Arrange / Act
    let style = TextBlockStyle::default();

    // Assert
    assert!((style.paragraph_spacing.to_pt() - 12.0).abs() < f32::EPSILON);
    assert_eq!(style.font_kind, FontKind::Serif);
  }

  #[test]
  fn validate_accepts_zero_paragraph_spacing() {
    // Arrange
    let style = TextBlockStyle {
      paragraph_spacing: Length::pt(0.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_paragraph_spacing() {
    // Arrange
    let style = TextBlockStyle {
      paragraph_spacing: Length::pt(-1.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
