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
  /// 表の上端・下端の罫線の太さ。`0pt` は引かず、罫線も `rule_gap` の空きも場所を取らない
  #[garde(custom(non_negative))]
  pub outer_rule_thickness: Length,
  /// ヘッダ下の罫線と `\row[separator=rule]` の罫線の太さ。`0pt` は引かず、罫線も `rule_gap` の空きも場所を取らない
  #[garde(custom(non_negative))]
  pub rule_thickness: Length,
  /// 罫線と行の間の空き（上端の罫線は下だけ、下端の罫線は上だけ、それ以外の罫線は上下）
  #[garde(custom(non_negative))]
  pub rule_gap: Length,
  /// `\row[separator=gap]` の空き
  #[garde(custom(non_negative))]
  pub row_gap: Length,
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
      outer_rule_thickness: Length::pt(0.8),
      rule_thickness: Length::pt(0.5),
      rule_gap: Length::pt(2.0),
      row_gap: Length::pt(5.0),
      rule_color: None,
      cell_padding: Length::pt(4.0),
      head_typeface: Typeface::SerifBold,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TableStyle;
  use crate::{document::Typeface, length::Length};

  #[test]
  fn rule_defaults_follow_booktabs() {
    let style = TableStyle::default();

    assert_eq!(style.outer_rule_thickness, Length::pt(0.8));
    assert_eq!(style.rule_thickness, Length::pt(0.5));
    assert_eq!(style.rule_gap, Length::pt(2.0));
    assert_eq!(style.row_gap, Length::pt(5.0));
  }

  #[test]
  fn validate_rejects_negative_rule_gap() {
    let style = TableStyle {
      rule_gap: Length::pt(-0.1),
      ..TableStyle::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_outer_rule_thickness() {
    let style = TableStyle {
      outer_rule_thickness: Length::pt(-0.1),
      ..TableStyle::default()
    };

    assert!(style.validate().is_err());
  }

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
