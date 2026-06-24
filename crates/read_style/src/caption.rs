//! 図・表のキャプションスタイル設定型。
//!
//! [`FigureStyle`](crate::FigureStyle) と [`TableStyle`](crate::TableStyle) が共有する 2 フィールド
//! （書式テンプレートとフォントサイズ）を [`CaptionStyle`] にまとめる。配置は図・表ともに
//! ソース上の `\caption` の出現位置で決まるため、スタイル側では持たず Document IR
//! （`document::CaptionPosition` / `DocNode::Figure` 等）が直接保持する。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::length::{Length, positive};

/// キャプションの共通設定（figure / table で共有）。
///
/// TOML 上では `[figure.caption]` / `[table.caption]` の各テーブルにマップされる。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct CaptionStyle {
  /// キャプションの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1), custom(crate::placeholder::caption_format))]
  pub format: String,
  /// キャプションのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
}

impl Default for CaptionStyle {
  /// 既定値: `"{number}: {title}"` / 11pt。
  ///
  /// `format` の文言は要素ごとに上書きする想定（`FigureStyle` は "Figure ..."、`TableStyle` は "Table ..."）。
  fn default() -> Self {
    return Self {
      format: "{number}: {title}".to_string(),
      font_size: Length::pt(11.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::length::Length;

  use super::CaptionStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(CaptionStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    let style = CaptionStyle {
      format: String::new(),
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
    // Arrange: format のみ指定
    let toml = "format = \"Figure {number}: {title}\"\n";

    // Act
    let style: CaptionStyle = toml::from_str(toml).unwrap();

    // Assert
    assert_eq!(style.format, "Figure {number}: {title}");
    assert!((style.font_size.to_pt() - 11.0).abs() < f32::EPSILON);
  }
}
