//! 表（table）環境のスタイル設定型。

use garde::Validate;
use serde::Deserialize;

use crate::{
  color::Color,
  document::Typeface,
  length::{Length, non_negative},
  style::{NumberTitleTemplate, caption::CaptionStyle},
};

/// 表のスタイル設定
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct TableStyle {
  /// キャプション本体（書式テンプレート・フォントサイズ・書体）
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
  pub head_typeface: Typeface,
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
      head_typeface: Typeface::SerifBold,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::TableStyle;
  use crate::document::Typeface;

  #[test]
  fn head_typeface_defaults_to_serif_bold() {
    let style = TableStyle::default();

    assert_eq!(style.head_typeface, Typeface::SerifBold);
  }

  #[test]
  fn deserialize_overrides_head_typeface() {
    let toml = "
head_typeface = \"sans_serif_bold\"
";
    let style: TableStyle = toml::from_str(toml).expect("`[table]` の本体として読めるはず");
    assert_eq!(style.head_typeface, Typeface::SansSerifBold);
  }
}
