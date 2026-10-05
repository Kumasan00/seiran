//! 数式のスタイル設定型（`[math]` テーブル）。

use garde::Validate;
use serde::Deserialize;

use crate::{
  length::{Length, non_negative},
  project::non_negative_finite,
  style::{BlockAlignment, NumberTemplate},
};

/// 数式設定全体（`[math]` テーブル）。
#[derive(Debug, Clone, Default, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathStyle {
  /// 上付き / 下付きスクリプトのスタイル（`[math.script]`）。インライン数式にも効く。
  #[garde(dive)]
  pub script: MathScriptStyle,
  /// 表示数式ブロックのレイアウトスタイル（`[math.block]`）。全表示数式環境が共有する。
  #[garde(dive)]
  pub block: MathBlockStyle,
}

/// スクリプト（上付き / 下付き）のシフト量の設定（`[math.script]`）
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathScriptStyle {
  /// 上付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で上方向）
  #[garde(custom(non_negative_finite))]
  pub superscript_raise_factor: f32,
  /// 下付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で下方向）
  #[garde(custom(non_negative_finite))]
  pub subscript_drop_factor: f32,
}

impl Default for MathScriptStyle {
  fn default() -> Self {
    return Self {
      superscript_raise_factor: 0.4,
      subscript_drop_factor: 0.2,
    };
  }
}

/// 表示数式ブロックのレイアウトスタイル（`[math.block]`）
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathBlockStyle {
  /// 式の横に出る数式番号（タグ）の書式テンプレート。`{number}` を発番番号で置換する（既定
  /// `"({number})"` → `"(1.1)"`）。`\ref{eq:x}` の表示
  /// （[`crate::style::counter::CounterStyle::ref_format`]）とは独立。
  #[garde(dive)]
  pub tag_format: NumberTemplate,
  /// 数式番号の配置側
  pub number_side: NumberSide,
  /// 数式本体の揃え
  pub alignment: BlockAlignment,
  /// 行間（隣り合う行のベースライン間に挿入する追加アキ）
  #[garde(custom(non_negative))]
  pub row_gap: Length,
  /// 列間（`&` で分割した列の間隔）
  #[garde(custom(non_negative))]
  pub column_gap: Length,
  /// 数式ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 数式ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for MathBlockStyle {
  fn default() -> Self {
    return Self {
      tag_format: NumberTemplate::parse("({number})"),
      number_side: NumberSide::Right,
      alignment: BlockAlignment::Center,
      row_gap: Length::pt(3.0),
      column_gap: Length::pt(6.0),
      top_margin: Length::pt(8.0),
      bottom_margin: Length::pt(8.0),
    };
  }
}

/// 数式番号を本体のどちら側に配置するか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub(crate) enum NumberSide {
  /// 数式の右側に番号を配置
  Right,
  /// 数式の左側に番号を配置
  Left,
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{MathBlockStyle, MathScriptStyle, NumberSide};
  use crate::style::BlockAlignment;

  #[test]
  fn block_default_uses_right_number_and_center_body() {
    let block = MathBlockStyle::default();

    assert_eq!(block.number_side, NumberSide::Right);
    assert_eq!(block.alignment, BlockAlignment::Center);
    assert_eq!(block.tag_format.as_str(), "({number})");
  }

  #[test]
  fn validate_rejects_negative_raise_factor() {
    let style = MathScriptStyle {
      superscript_raise_factor: -0.1,
      ..MathScriptStyle::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_raise_factor() {
    let style = MathScriptStyle {
      superscript_raise_factor: 0.0,
      ..MathScriptStyle::default()
    };

    assert!(style.validate().is_ok());
  }

  #[test]
  fn rejects_renamed_equation_table_keys() {
    assert!(toml::from_str::<MathBlockStyle>("number_format = \"({number})\"\n").is_err());
  }
}
