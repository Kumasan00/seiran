//! リスト要素（順序付き / 順序なし）のスタイル設定型。

use garde::Validate;
use model::{
  FontKind,
  length::{Length, non_negative},
};
use serde::{Deserialize, Serialize};

/// リスト要素のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct ListStyle {
  /// 各リスト項目の左インデント量
  #[garde(custom(non_negative))]
  pub indent: Length,
  /// リスト項目間の下余白
  #[garde(custom(non_negative))]
  pub item_margin_bottom: Length,
  /// 順序なしリストのマーカー文字列（例: `"•"`）。後ろに自動で半角スペースが付与される。
  #[garde(length(chars, min = 1))]
  pub unordered_marker: String,
  /// 順序付きリストのマーカー書式（例: `"{number}."`）。`{number}` は 1 始まりの項目番号で置換される。
  /// 後ろに自動で半角スペースが付与される。
  #[garde(length(chars, min = 1), custom(crate::style::placeholder::ordered_list_format))]
  pub ordered_marker_format: String,
  /// マーカー描画に使用するフォント種別
  pub marker_font_kind: FontKind,
}

impl Default for ListStyle {
  fn default() -> Self {
    return Self {
      indent: Length::pt(20.0),
      item_margin_bottom: Length::pt(4.0),
      unordered_marker: "•".to_string(),
      ordered_marker_format: "{number}.".to_string(),
      marker_font_kind: FontKind::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use model::{FontKind, length::Length};

  use super::ListStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(ListStyle::default().validate().is_ok());
  }

  #[test]
  fn default_matches_documented_values() {
    // Arrange / Act
    let style = ListStyle::default();

    // Assert
    assert!((style.indent.to_pt() - 20.0).abs() < f32::EPSILON);
    assert!((style.item_margin_bottom.to_pt() - 4.0).abs() < f32::EPSILON);
    assert_eq!(style.unordered_marker, "•");
    assert_eq!(style.ordered_marker_format, "{number}.");
    assert_eq!(style.marker_font_kind, FontKind::Serif);
  }

  #[test]
  fn validate_rejects_negative_indent() {
    // Arrange
    let style = ListStyle {
      indent: Length::pt(-1.0),
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
  fn rejects_renamed_ordered_format_key() {
    // Arrange: 旧キー `ordered_format` はハードリネームで廃止された（→ `ordered_marker_format`）。
    // `deny_unknown_fields` が未知フィールドとして弾く（移行: 旧 config は即エラー）。
    let toml = "
indent = \"20pt\"
item_margin_bottom = \"4pt\"
unordered_marker = \"•\"
ordered_format = \"{number}.\"
marker_font_kind = \"serif\"
";

    // Act
    let result: Result<ListStyle, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "旧キー `ordered_format` は未知フィールドとして拒否される");
  }
}
