//! ディスプレイ数式（equation）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::primitives::length::{Length, non_negative};

/// ディスプレイ数式のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct EquationStyle {
  /// 数式番号の書式テンプレート。`{number}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub number_format: String,
  /// 数式番号の配置側
  pub number_side: NumberSide,
  /// 数式本体の揃え
  pub alignment: Alignment,
  /// 数式ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 数式ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for EquationStyle {
  fn default() -> Self {
    return Self {
      number_format: "({number})".to_string(),
      number_side: NumberSide::Right,
      alignment: Alignment::Center,
      top_margin: Length::pt(8.0),
      bottom_margin: Length::pt(8.0),
    };
  }
}

/// 数式番号を本体のどちら側に配置するか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum NumberSide {
  /// 数式の右側に番号を配置
  Right,
  /// 数式の左側に番号を配置
  Left,
}

/// 数式本体の揃え
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum Alignment {
  /// 中央揃え
  Center,
  /// 左揃え
  Left,
  /// 右揃え
  Right,
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{Alignment, EquationStyle, NumberSide};

  #[test]
  fn validate_accepts_default() {
    assert!(EquationStyle::default().validate().is_ok());
  }

  #[test]
  fn default_uses_right_number_and_center_body() {
    let style = EquationStyle::default();
    assert_eq!(style.number_side, NumberSide::Right);
    assert_eq!(style.alignment, Alignment::Center);
  }

  #[test]
  fn validate_rejects_empty_number_format() {
    let style = EquationStyle {
      number_format: String::new(),
      ..EquationStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
