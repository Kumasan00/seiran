//! リスト要素（順序付き / 順序なし）のスタイル設定型。
//!
//! `crates/layout/src/lowering/list.rs` で使用されるインデント量・項目間余白・
//! マーカー書式・マーカーフォント種別をまとめて定義します。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::FontKindConfig;

/// リスト要素のスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct ListStyle {
  /// 各リスト項目の左インデント量（pt）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub indent: f32,
  /// リスト項目間の下余白（pt）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub item_margin_bottom: f32,
  /// 順序なしリストのマーカー文字列（例: `"•"`）。
  /// 後ろに自動で半角スペースが付与される。
  #[garde(length(chars, min = 1))]
  pub unordered_marker: String,
  /// 順序付きリストのマーカー書式（例: `"{number}."`）。
  /// `{number}` は 1 始まりの項目番号で置換される。後ろに自動で半角スペースが付与される。
  #[garde(length(chars, min = 1))]
  pub ordered_format: String,
  /// マーカー描画に使用するフォント種別
  pub marker_font_kind: FontKindConfig,
}

impl Default for ListStyle {
  fn default() -> Self {
    // 既定値は `crates/layout/src/lowering/list.rs` の現行ハードコード値と一致させる
    return Self {
      indent: 20.0,
      item_margin_bottom: 4.0,
      unordered_marker: "•".to_string(),
      ordered_format: "{number}.".to_string(),
      marker_font_kind: FontKindConfig::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{FontKindConfig, ListStyle};

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(ListStyle::default().validate().is_ok());
  }

  #[test]
  fn default_matches_current_hardcoded_values() {
    // Arrange / Act
    let style = ListStyle::default();

    // Assert: layout 側のハードコードと完全一致していること
    assert!((style.indent - 20.0).abs() < f32::EPSILON);
    assert!((style.item_margin_bottom - 4.0).abs() < f32::EPSILON);
    assert_eq!(style.unordered_marker, "•");
    assert_eq!(style.ordered_format, "{number}.");
    assert_eq!(style.marker_font_kind, FontKindConfig::Serif);
  }

  #[test]
  fn validate_rejects_negative_indent() {
    // Arrange
    let style = ListStyle {
      indent: -1.0,
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_item_margin_bottom() {
    // Arrange
    let style = ListStyle {
      item_margin_bottom: -0.1,
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_unordered_marker() {
    // Arrange
    let style = ListStyle {
      unordered_marker: String::new(),
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_ordered_format() {
    // Arrange
    let style = ListStyle {
      ordered_format: String::new(),
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_indent() {
    // Arrange: range(min = 0.0) なので 0.0 は許容される
    let style = ListStyle {
      indent: 0.0,
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
