//! 本文段落のスタイル設定型。
//!
//! 見出しレベルの `paragraph`（[`HeadingStyle`]）と区別するため `ParagraphStyle` は
//! `Style::body` フィールドに置かれる。`crates/layout/src/lowering/paragraph.rs` で
//! 段落本文のフォント種別および段落間スペースを参照する想定。
//!
//! [`HeadingStyle`]: crate::HeadingStyle

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::FontKindConfig;

/// 本文段落のスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct ParagraphStyle {
  /// 段落末に挿入するスペース（pt）。
  ///
  /// `None`（未指定）の場合は `Style::font_size` を使う既存挙動を温存する。
  /// 明示的に値を指定した場合のみ、その値で固定する。
  #[garde(skip)]
  pub inter_paragraph_spacing: Option<f32>,
  /// 段落本文のフォント種別
  pub font_kind: FontKindConfig,
}

impl Default for ParagraphStyle {
  fn default() -> Self {
    // 既定値は `crates/layout/src/lowering/paragraph.rs` の現行挙動と一致させる
    // （inter_paragraph_spacing = None → font_size を使う）
    return Self {
      inter_paragraph_spacing: None,
      font_kind: FontKindConfig::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{FontKindConfig, ParagraphStyle};

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(ParagraphStyle::default().validate().is_ok());
  }

  #[test]
  fn default_matches_current_hardcoded_values() {
    // Arrange / Act
    let style = ParagraphStyle::default();

    // Assert
    assert!(style.inter_paragraph_spacing.is_none());
    assert_eq!(style.font_kind, FontKindConfig::Serif);
  }

  #[test]
  fn validate_accepts_explicit_inter_paragraph_spacing() {
    // Arrange
    let style = ParagraphStyle {
      inter_paragraph_spacing: Some(20.0),
      ..ParagraphStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
