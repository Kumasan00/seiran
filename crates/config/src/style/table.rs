//! 表（table）環境のスタイル設定型。

use garde::Validate;
use model::{
  Color,
  length::{Length, non_negative},
};
use serde::{Deserialize, Serialize};

use crate::style::caption::CaptionStyle;

/// 表のスタイル設定
///
/// キャプション位置は図と同様、ソース上の `\caption` の出現位置で決まるためスタイル側では持たない。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TableStyle {
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
}

impl Default for TableStyle {
  fn default() -> Self {
    return Self {
      caption: CaptionStyle {
        format: "Table {number}: {title}".to_string(),
        font_size: Length::pt(11.0),
      },
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
      rule_thickness: Length::pt(0.5),
      rule_color: None,
      cell_padding: Length::pt(4.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use model::Length;

  use super::TableStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(TableStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    let style = TableStyle {
      rule_thickness: Length::pt(-0.1),
      ..TableStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
