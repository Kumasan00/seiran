//! 本文段落（`DocNode::Paragraph`）のスタイル設定型。
//!
//! 見出しレベルの `Paragraph` と区別するため、本文段落は `TextBlockStyle` という名前にし、
//! `Style::text` フィールドに置く。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{
  FontKind,
  length::{Length, non_negative, positive},
};

/// 本文段落のスタイル設定
///
/// 本文テキストの既定見た目（フォントサイズ・行高・段落間隔・字下げ・書体）を 1 か所に集約する。
///
/// `font_size` は本文の既定フォントサイズ（既定 12pt）。見出し・キャプション・参照などは各自が
/// 個別の `font_size` を持ち、本フィールドは「本文段落の既定」を表す。
/// `line_height_factor` は行高（フォントサイズに対する倍率、既定 1.2）。
///
/// `paragraph_spacing` は段落末に挿入するスペース。
/// 旧 `inter_paragraph_spacing: Option<f32>` の `None`（未指定 → `font_size`）挙動を廃止し、
/// 明示的な既定値（12.0pt）を使う。サイズを動的に追従させたければ呼び出し側で計算する。
///
/// `first_line_indent` は段落先頭行の字下げ量。既定 0pt（字下げなし＝従来のブロック段落方式）。
/// 正の値のとき各段落の先頭行だけが字下げされる（折り返し 2 行目以降・`\\` 直後の行には効かない）。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TextBlockStyle {
  /// 本文の既定フォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 行高（フォントサイズに対する倍率）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 段落末に挿入するスペース
  #[garde(custom(non_negative))]
  pub paragraph_spacing: Length,
  /// 段落先頭行の字下げ量（既定 0pt = 字下げなし）
  #[garde(custom(non_negative))]
  pub first_line_indent: Length,
  /// 段落本文のフォント種別
  pub font_kind: FontKind,
}

impl Default for TextBlockStyle {
  fn default() -> Self {
    return Self {
      font_size: Length::pt(12.0),
      line_height_factor: 1.2,
      paragraph_spacing: Length::pt(12.0),
      first_line_indent: Length::pt(0.0),
      font_kind: FontKind::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{FontKind, length::Length};

  use super::TextBlockStyle;

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
    assert!((style.font_size.to_pt() - 12.0).abs() < f32::EPSILON);
    assert!((style.line_height_factor - 1.2).abs() < f32::EPSILON);
    assert!((style.paragraph_spacing.to_pt() - 12.0).abs() < f32::EPSILON);
    assert!((style.first_line_indent.to_pt() - 0.0).abs() < f32::EPSILON);
    assert_eq!(style.font_kind, FontKind::Serif);
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = TextBlockStyle {
      font_size: Length::pt(0.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    // Arrange
    let style = TextBlockStyle {
      line_height_factor: 0.0,
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_first_line_indent() {
    // Arrange
    let style = TextBlockStyle {
      first_line_indent: Length::pt(-1.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
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
