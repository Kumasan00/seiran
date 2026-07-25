//! リスト要素（順序付き / 順序なし）のスタイル設定型。

use garde::Validate;
use model::{
  FontKind,
  length::{Length, non_negative},
};
use serde::{Deserialize, Serialize};

use crate::style::number_style::NumberStyle;

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
  /// ネスト段（深さ 1 以上）の unordered マーカー系列。深さ `d`（≥1）は `(d - 1) % N` で循環的に引く
  /// （`N` は要素数）。最上位（深さ 0）の `unordered_marker` とは独立。
  #[garde(length(min = 1), inner(length(chars, min = 1)))]
  pub nested_unordered_markers: Vec<String>,
  /// ネスト段（深さ 1 以上）の ordered マーカー系列。循環規則は [`Self::nested_unordered_markers`] と同じ。
  /// 最上位（深さ 0）の `ordered_marker_format` とは独立。
  #[garde(length(min = 1), dive)]
  pub nested_ordered_formats: Vec<NestedOrderedFormat>,
}

impl Default for ListStyle {
  fn default() -> Self {
    return Self {
      indent: Length::pt(20.0),
      item_margin_bottom: Length::pt(4.0),
      unordered_marker: "•".to_string(),
      ordered_marker_format: "{number}.".to_string(),
      marker_font_kind: FontKind::Serif,
      nested_unordered_markers: vec!["–".to_string(), "*".to_string(), "·".to_string()],
      nested_ordered_formats: vec![
        NestedOrderedFormat {
          number_style: NumberStyle::AlphaLower,
          format: "({number})".to_string(),
        },
        NestedOrderedFormat {
          number_style: NumberStyle::RomanLower,
          format: "{number}.".to_string(),
        },
        NestedOrderedFormat {
          number_style: NumberStyle::AlphaUpper,
          format: "{number}.".to_string(),
        },
      ],
    };
  }
}

/// ネスト段（深さ 1 以上）1 段分の ordered マーカー書式（番号書式 + 装飾テンプレート）
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct NestedOrderedFormat {
  /// この段の番号の数字表記スタイル
  pub number_style: NumberStyle,
  /// `{number}` を含む装飾テンプレート（例: `"({number})"`）。後ろに自動で半角スペースが付与される。
  #[garde(length(chars, min = 1), custom(crate::style::placeholder::ordered_list_format))]
  pub format: String,
}

impl Default for NestedOrderedFormat {
  fn default() -> Self {
    return Self {
      number_style: NumberStyle::default(),
      format: "{number}.".to_string(),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use model::{FontKind, length::Length};

  use super::{ListStyle, NestedOrderedFormat};
  use crate::style::number_style::NumberStyle;

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
    assert_eq!(style.nested_unordered_markers, vec!["–", "*", "·"]);
    let formats: Vec<(NumberStyle, &str)> =
      style.nested_ordered_formats.iter().map(|f| return (f.number_style, f.format.as_str())).collect();
    assert_eq!(
      formats,
      vec![
        (NumberStyle::AlphaLower, "({number})"),
        (NumberStyle::RomanLower, "{number}."),
        (NumberStyle::AlphaUpper, "{number}."),
      ]
    );
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
    // Arrange
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

  #[test]
  fn validate_rejects_empty_nested_unordered_markers() {
    // Arrange
    let style = ListStyle {
      nested_unordered_markers: Vec::new(),
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_string_in_nested_unordered_markers() {
    // Arrange
    let style = ListStyle {
      nested_unordered_markers: vec!["–".to_string(), String::new()],
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_nested_ordered_formats() {
    // Arrange
    let style = ListStyle {
      nested_ordered_formats: Vec::new(),
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_nested_ordered_format_with_unknown_placeholder() {
    // Arrange
    let style = ListStyle {
      nested_ordered_formats: vec![NestedOrderedFormat {
        number_style: NumberStyle::AlphaLower,
        format: "{title}".to_string(),
      }],
      ..ListStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
