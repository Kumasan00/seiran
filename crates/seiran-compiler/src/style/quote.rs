//! 引用ブロック（`quote` / `quotation`）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  document::FontKind,
  length::{Length, non_negative},
};

/// 引用ブロックのスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct QuoteStyle {
  /// 本文左右端からのブロック字下げ量（左右に同量適用）
  #[garde(custom(non_negative))]
  pub indent: Length,
  /// ブロック上のアキ
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// ブロック下のアキ
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// `quotation` のブロック内段落先頭行の字下げ量（`quote` では未使用）
  #[garde(custom(non_negative))]
  pub first_line_indent: Length,
  /// ブロック本文の既定フォント種別
  pub font_kind: FontKind,
}

impl Default for QuoteStyle {
  fn default() -> Self {
    return Self {
      indent: Length::pt(20.0),
      top_margin: Length::pt(6.0),
      bottom_margin: Length::pt(6.0),
      first_line_indent: Length::pt(15.0),
      font_kind: FontKind::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::QuoteStyle;
  use crate::{document::FontKind, length::Length};

  #[test]
  fn default_matches_documented_values() {
    // Arrange / Act
    let style = QuoteStyle::default();

    // Assert
    assert!((style.indent.to_pt() - 20.0).abs() < f32::EPSILON);
    assert!((style.top_margin.to_pt() - 6.0).abs() < f32::EPSILON);
    assert!((style.bottom_margin.to_pt() - 6.0).abs() < f32::EPSILON);
    assert!((style.first_line_indent.to_pt() - 15.0).abs() < f32::EPSILON);
    assert_eq!(style.font_kind, FontKind::Serif);
  }

  #[test]
  fn validate_rejects_negative_indent() {
    // Arrange
    let style = QuoteStyle {
      indent: Length::pt(-1.0),
      ..QuoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_first_line_indent() {
    // Arrange
    let style = QuoteStyle {
      first_line_indent: Length::pt(-1.0),
      ..QuoteStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
