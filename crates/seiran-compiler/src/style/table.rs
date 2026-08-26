//! 表（table）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  color::Color,
  document::FontKind,
  length::{Length, non_negative},
  style::{NumberTitleTemplate, caption::CaptionStyle},
};

/// 表のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct TableStyle {
  /// キャプション本体（書式テンプレートとフォントサイズ）
  #[garde(dive)]
  pub caption: CaptionStyle,
  /// 表ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 表ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 表本体とキャプションの間隔
  #[garde(custom(non_negative))]
  pub inner_margin: Length,
  /// 罫線の太さ
  #[garde(custom(non_negative))]
  pub rule_thickness: Length,
  /// 罫線色。`None` は黒
  pub rule_color: Option<Color>,
  /// セル内容の左右内側余白（各セルの両側に適用される）
  #[garde(custom(non_negative))]
  pub cell_padding: Length,
  /// ヘッダ行（`\head{}`）セルの書体
  pub head_font_kind: FontKind,
}

impl Default for TableStyle {
  fn default() -> Self {
    return Self {
      caption: CaptionStyle {
        format: NumberTitleTemplate::parse("Table {number}: {title}"),
        ..CaptionStyle::default()
      },
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
      rule_thickness: Length::pt(0.5),
      rule_color: None,
      cell_padding: Length::pt(4.0),
      head_font_kind: FontKind::SerifBold,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TableStyle;
  use crate::{document::FontKind, length::Length};

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    let style = TableStyle {
      rule_thickness: Length::pt(-0.1),
      ..TableStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn head_font_kind_defaults_to_serif_bold() {
    let style = TableStyle::default();

    assert_eq!(style.head_font_kind, FontKind::SerifBold);
  }

  #[test]
  fn deserialize_overrides_head_font_kind() {
    // Arrange
    let toml = "
head_font_kind = \"sans_serif_bold\"
";

    // Act
    let style: TableStyle = toml::from_str(toml).expect("`[table]` の本体として読めるはず");

    // Assert
    assert_eq!(style.head_font_kind, FontKind::SansSerifBold);
  }
}
