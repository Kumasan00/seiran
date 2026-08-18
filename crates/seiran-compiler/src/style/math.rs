//! 数式のスタイル設定型（`[math]` テーブル）。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  length::{Length, non_negative, positive},
  style::NumberTemplate,
};

/// 数式設定全体（`[math]` テーブル）。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
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

impl Default for MathStyle {
  fn default() -> Self {
    return Self {
      script: MathScriptStyle::default(),
      block: MathBlockStyle::default(),
    };
  }
}

/// スクリプト（上付き / 下付き）のスタイル設定（`[math.script]`）
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathScriptStyle {
  /// 上付き / 下付きスクリプトのフォントサイズ倍率（親フォントサイズに対する比）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub script_size_factor: f32,
  /// 上付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で上方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub superscript_raise_factor: f32,
  /// 下付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で下方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub subscript_drop_factor: f32,
  /// スクリプトフォントサイズの下限。極端な縮小を防ぐためのクランプ値
  #[garde(custom(positive))]
  pub min_script_font_size: Length,
}

impl Default for MathScriptStyle {
  fn default() -> Self {
    return Self {
      script_size_factor: 0.7,
      superscript_raise_factor: 0.4,
      subscript_drop_factor: 0.2,
      min_script_font_size: Length::pt(6.0),
    };
  }
}

/// 表示数式ブロックのレイアウトスタイル（`[math.block]`）
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathBlockStyle {
  /// 式の横に出る数式番号（タグ）の書式テンプレート。`{number}` を発番番号で置換する（既定
  /// `"({number})"` → `"(1.1)"`）。これは番号 3 系統のうち **tag**（式の横に出す）で、番号を構築する
  /// `counters.equation`（**number**）や `\ref{eq:x}` の表示を決める
  /// [`crate::style::CounterStyle::ref_format`]（**ref**）とは別物。既定値が一致するのは LaTeX 慣習で
  /// 「式の横」も「素の相互参照」も括弧付き番号だからで、両者は独立に変更できる。
  #[garde(dive)]
  pub tag_format: NumberTemplate,
  /// 数式番号の配置側
  pub number_side: NumberSide,
  /// 数式本体の揃え
  pub alignment: Alignment,
  /// 行間（隣り合う行のベースライン間に挿入する追加アキ、pt）
  #[garde(custom(non_negative))]
  pub row_gap: Length,
  /// 列間（`&` で分割した列の間隔、pt）
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
      alignment: Alignment::Center,
      row_gap: Length::pt(3.0),
      column_gap: Length::pt(6.0),
      top_margin: Length::pt(8.0),
      bottom_margin: Length::pt(8.0),
    };
  }
}

/// 数式番号を本体のどちら側に配置するか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub(crate) enum NumberSide {
  /// 数式の右側に番号を配置
  Right,
  /// 数式の左側に番号を配置
  Left,
}

/// 数式本体の揃え
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub(crate) enum Alignment {
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

  use super::{Alignment, MathBlockStyle, MathScriptStyle, MathStyle, NumberSide};
  use crate::style::NumberTemplate;

  #[test]
  fn block_default_uses_right_number_and_center_body() {
    // Arrange / Act
    let block = MathBlockStyle::default();

    // Assert
    assert_eq!(block.number_side, NumberSide::Right);
    assert_eq!(block.alignment, Alignment::Center);
    assert_eq!(block.tag_format.as_str(), "({number})");
  }

  #[test]
  fn validate_rejects_empty_tag_format() {
    // Arrange
    let mut style = MathStyle::default();
    style.block.tag_format = NumberTemplate::parse("");

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_script_size_factor() {
    // Arrange
    let style = MathScriptStyle {
      script_size_factor: 0.0,
      ..MathScriptStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_raise_factor() {
    // Arrange
    let style = MathScriptStyle {
      superscript_raise_factor: -0.1,
      ..MathScriptStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_raise_factor() {
    // Arrange
    let style = MathScriptStyle {
      superscript_raise_factor: 0.0,
      ..MathScriptStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn rejects_renamed_equation_table_keys() {
    assert!(toml::from_str::<MathBlockStyle>("number_format = \"({number})\"\n").is_err());
  }
}
