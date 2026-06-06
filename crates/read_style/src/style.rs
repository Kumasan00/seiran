//! スタイル設定全体を保持する [`Style`] 構造体。
//!
//! [`crate::core::CoreStyle`] と [`crate::extended::ExtendedStyle`] の 2 層に分かれている。
//! `core` は `lowering` / `pdf_gen` から参照される実働フィールド、`extended` は資産として
//! 保持している未参照フィールド（脚注・目次・参考文献等）を含む。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::HeadingLevel;

use crate::{
  core::{
    CoreStyle,
    counter::{CounterName, CounterStyle},
    heading::HeadingStyle,
  },
  extended::ExtendedStyle,
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
///
/// TOML 上では `core` が `#[serde(flatten)]` で展開され、`extended` のみが
/// `[extended.*]` 配下のサブテーブルとして現れる。
#[derive(Debug, Clone, Default, Deserialize, Serialize, Validate)]
#[serde(default)]
pub struct Style {
  /// `lowering` / `pdf_gen` から参照されるコアフィールド一式
  #[serde(flatten)]
  #[garde(dive)]
  pub core: CoreStyle,
  /// 未参照領域（脚注・目次・ハイパーリンク・参考文献）。`[extended.*]` 配下にマップされる
  #[garde(dive)]
  pub extended: ExtendedStyle,
}

impl Style {
  /// 指定された見出しレベルの [`HeadingStyle`] への不変参照を返す
  ///
  /// `style.core.heading[level]` でも同じことができるが、`heading` の内部表現を変えても
  /// 呼び出し側に影響しないよう、利用側にはこの accessor を使ってもらう。
  #[must_use]
  pub fn heading(&self, level: HeadingLevel) -> &HeadingStyle { return &self.core.heading[level]; }

  /// 指定された名前のカウンタ定義への不変参照を返す（9 種固定のため必ず存在する）。
  #[must_use]
  pub fn counter(&self, name: CounterName) -> &CounterStyle { return self.core.counters.get(name); }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{HeadingLevel, length::Length};

  use super::Style;
  use crate::{Color, core::counter::CounterName};

  #[test]
  fn validate_accepts_default() {
    assert!(Style::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let mut style = Style::default();
    style.core.font_size = Length::pt(0.0);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    let mut style = Style::default();
    style.core.line_height_factor = 0.0;
    assert!(style.validate().is_err());
  }

  #[test]
  fn heading_accessor_returns_correct_level() {
    let style = Style::default();
    assert!(style.heading(HeadingLevel::Part).font_size > style.heading(HeadingLevel::Section).font_size);
  }

  #[test]
  fn counter_accessor_finds_figure() {
    let style = Style::default();
    assert_eq!(style.counter(CounterName::Figure).display_name, "Figure");
  }

  #[test]
  fn default_background_color_is_none() {
    let style = Style::default();
    assert!(style.core.background_color.is_none());
  }

  #[test]
  fn validate_dives_into_nested_table_rule_thickness() {
    let mut style = Style::default();
    style.core.table.rule_thickness = Length::pt(-0.1);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_counters_display_name() {
    let mut style = Style::default();
    style.core.counters.figure.display_name = String::new();
    assert!(style.validate().is_err());
  }

  #[test]
  fn background_color_field_accepts_color_value() {
    let mut style = Style::default();
    style.core.background_color = Some(Color::new(204, 179, 153));
    assert!(style.validate().is_ok());
    let color = style.core.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [204, 179, 153]);
  }
}
