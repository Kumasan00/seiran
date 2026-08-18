//! タイトルページ（`\maketitle` 相当）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  document::FontKind,
  length::{Length, non_negative, positive},
};

/// タイトルページのスタイル設定。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct TitlePageStyle {
  /// タイトルページを生成するか（既定 `false` = 無効）
  #[garde(skip)]
  pub enabled: bool,
  /// ページ上端からタイトルまでの送り（垂直位置の簡易指定）
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// タイトルのフォントサイズ
  #[garde(custom(positive))]
  pub title_font_size: Length,
  /// タイトルのフォント種別
  #[garde(skip)]
  pub title_font_kind: FontKind,
  /// タイトルの下の縦アキ（タイトルと著者の間隔）
  #[garde(custom(non_negative))]
  pub title_bottom_margin: Length,
  /// 著者のフォントサイズ
  #[garde(custom(positive))]
  pub author_font_size: Length,
  /// 著者のフォント種別
  #[garde(skip)]
  pub author_font_kind: FontKind,
  /// 著者の下の縦アキ（著者と日付の間隔）
  #[garde(custom(non_negative))]
  pub author_bottom_margin: Length,
  /// 日付のフォントサイズ
  #[garde(custom(positive))]
  pub date_font_size: Length,
  /// 日付のフォント種別
  #[garde(skip)]
  pub date_font_kind: FontKind,
}

impl Default for TitlePageStyle {
  fn default() -> Self {
    return Self {
      enabled: false,
      top_margin: Length::pt(120.0),
      title_font_size: Length::pt(36.0),
      title_font_kind: FontKind::SerifBold,
      title_bottom_margin: Length::pt(24.0),
      author_font_size: Length::pt(18.0),
      author_font_kind: FontKind::Serif,
      author_bottom_margin: Length::pt(12.0),
      date_font_size: Length::pt(14.0),
      date_font_kind: FontKind::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TitlePageStyle;
  use crate::{document::FontKind, length::Length};

  #[test]
  fn default_is_disabled() {
    // Arrange / Act
    let style = TitlePageStyle::default();

    // Assert
    assert!(!style.enabled);
    assert_eq!(style.title_font_kind, FontKind::SerifBold);
    assert!((style.title_font_size.to_pt() - 36.0).abs() < f32::EPSILON);
  }

  #[test]
  fn deserializes_partial_with_defaults() {
    // Arrange
    let toml = "enabled = true\n";

    // Act
    let style: TitlePageStyle = toml::from_str(toml).unwrap();

    // Assert
    assert!(style.enabled);
    assert!((style.author_font_size.to_pt() - 18.0).abs() < f32::EPSILON);
  }

  #[test]
  fn validate_rejects_zero_title_font_size() {
    // Arrange
    let style = TitlePageStyle {
      title_font_size: Length::pt(0.0),
      ..TitlePageStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    // Arrange
    let style = TitlePageStyle {
      top_margin: Length::pt(-1.0),
      ..TitlePageStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_bottom_margin() {
    // Arrange
    let style = TitlePageStyle {
      title_bottom_margin: Length::pt(0.0),
      author_bottom_margin: Length::pt(0.0),
      ..TitlePageStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
