//! スタイル設定全体を保持する [`Style`] 構造体と既定値の定義。

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::HeadingLevel;

use crate::{
  Color,
  block::{
    heading::{HeadingStyle, default_per_level, deserialize_per_level},
    list::ListStyle,
    math::MathScriptStyle,
    text::TextBlockStyle,
  },
  common::{
    counter::{self, CounterEntry},
    length::{Length, positive},
    per_level::PerLevel,
  },
  extended::ExtendedStyle,
  float::{figure::FigureStyle, table::TableStyle},
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct Style {
  /// 本文の既定フォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 行高（フォントサイズに対する倍率）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 背景色。`None` は背景描画なし
  #[garde(skip)]
  pub background_color: Option<Color>,
  /// 見出し全 6 レベルのスタイル
  ///
  /// レベル別の既定値は [`default_per_level`] が供給する。TOML で `[heading.section]` のように
  /// 一部だけ書いた場合、欠落レベルは [`crate::block::heading::default_for_level`] で埋まる。
  #[garde(skip)]
  #[serde(default = "default_per_level", deserialize_with = "deserialize_per_level")]
  pub heading: PerLevel<HeadingStyle>,
  /// 本文段落のスタイル
  #[garde(dive)]
  pub text: TextBlockStyle,
  /// リストのスタイル
  #[garde(dive)]
  pub list: ListStyle,
  /// 数式レイアウトのスタイル
  #[garde(dive)]
  pub math: MathScriptStyle,
  /// 表のスタイル
  #[garde(dive)]
  pub table: TableStyle,
  /// 図フロートのスタイル
  #[garde(dive)]
  pub figure: FigureStyle,
  /// カウンタ定義テーブル（`[counters.<name>]`）。
  /// `parser::evaluator::counter::CounterRegistry::from_style` が読み取って実行時表現に変換する。
  /// 値は [`CounterEntry`] で「カウンタ定義」「別名」を排他的に表す。
  #[garde(dive)]
  pub counters: HashMap<String, CounterEntry>,
  /// 現状 `lowering/pdf_gen` 未参照のスタイル群（figure / equation / footnote / toc /
  /// hyperref / reference）。資産として保持し、TOML 上では `[extended.*]` 配下に書く。
  #[garde(dive)]
  pub extended: ExtendedStyle,
}

impl Default for Style {
  fn default() -> Self {
    return Self {
      font_size: Length::pt(12.0),
      line_height_factor: 1.2,
      background_color: None,
      heading: default_per_level(),
      text: TextBlockStyle::default(),
      list: ListStyle::default(),
      math: MathScriptStyle::default(),
      table: TableStyle::default(),
      figure: FigureStyle::default(),
      counters: counter::default_counters(),
      extended: ExtendedStyle::default(),
    };
  }
}

impl Style {
  /// 指定された見出しレベルの [`HeadingStyle`] への不変参照を返す
  ///
  /// `style.heading[level]` でも同じことができるが、`heading` の内部表現を変えても
  /// 呼び出し側に影響しないよう、利用側にはこの accessor を使ってもらう。
  #[must_use]
  pub fn heading(&self, level: HeadingLevel) -> &HeadingStyle { return &self.heading[level]; }

  /// 指定された名前のカウンタエントリ（定義または別名）への不変参照を返す。
  ///
  /// 別名解決（[`CounterEntry::Alias`](crate::CounterEntry::Alias) を辿って canonical な定義に
  /// 到達する）は呼び出し側の責任。
  #[must_use]
  pub fn counter(&self, name: &str) -> Option<&CounterEntry> { return self.counters.get(name); }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::HeadingLevel;

  use super::Style;
  use crate::{Color, common::length::Length};

  #[test]
  fn validate_accepts_default() {
    assert!(Style::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = Style {
      font_size: Length::pt(0.0),
      ..Style::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    let style = Style {
      line_height_factor: 0.0,
      ..Style::default()
    };
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
    assert!(style.counter("figure").is_some());
    assert!(style.counter("nonexistent").is_none());
  }

  #[test]
  fn default_background_color_is_none() {
    let style = Style::default();
    assert!(style.background_color.is_none());
  }

  #[test]
  fn validate_dives_into_nested_table_rule_thickness() {
    let mut style = Style::default();
    style.table.rule_thickness = Length::pt(-0.1);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_counters_display_name() {
    use crate::common::counter::CounterEntry;
    let mut style = Style::default();
    if let Some(CounterEntry::Counter(def)) = style.counters.get_mut("figure") {
      def.display_name = String::new();
    }
    assert!(style.validate().is_err());
  }

  #[test]
  fn background_color_field_accepts_color_value() {
    let style = Style {
      background_color: Some(Color::new(204, 179, 153)),
      ..Style::default()
    };
    assert!(style.validate().is_ok());
    let color = style.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [204, 179, 153]);
  }
}
