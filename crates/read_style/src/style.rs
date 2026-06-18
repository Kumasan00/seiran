//! スタイル設定全体を保持する [`Style`] 構造体。
//!
//! `lowering` / `pdf_gen` から実際に参照される本文・見出し・リスト・数式・図表・目次など、
//! 描画パイプラインに直接効くフィールドをトップレベルに集約する。サブスタイル型
//! （[`crate::caption::CaptionStyle`] 等）はクレート直下の各モジュールに置く。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{
  Color, HeadingLevel,
  length::{Length, positive},
};

use crate::{
  counter::{CounterName, CounterStyle, Counters},
  equation::EquationStyle,
  figure::FigureStyle,
  heading::{HeadingStyle, HeadingStyles},
  hyperref::HyperrefStyle,
  list::ListStyle,
  math::MathScriptStyle,
  page_numbering::PageNumbering,
  reference::ReferenceStyle,
  running::RunningContentStyle,
  table::TableStyle,
  text::TextBlockStyle,
  title_page::TitlePageStyle,
  toc::TocStyle,
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
///
/// TOML キーはそのままトップレベル（`font_size` / `[heading.section]` / `[figure.caption]` 等）に出る。
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
  /// レベル別の既定値は [`HeadingStyles::default`] が供給する。TOML で `[heading.section]` のように
  /// 一部だけ書いた場合、欠落レベルは [`crate::heading::default_for_level`] で埋まる。
  #[garde(skip)]
  pub heading: HeadingStyles,
  /// 本文段落のスタイル
  #[garde(dive)]
  pub text: TextBlockStyle,
  /// リストのスタイル
  #[garde(dive)]
  pub list: ListStyle,
  /// 表のスタイル
  #[garde(dive)]
  pub table: TableStyle,
  /// 図フロートのスタイル
  #[garde(dive)]
  pub figure: FigureStyle,
  /// ディスプレイ数式のスタイル
  #[garde(dive)]
  pub equation: EquationStyle,
  /// 数式レイアウトのスタイル
  #[garde(dive)]
  pub math: MathScriptStyle,
  /// カウンタ定義テーブル（`[counters.<name>]`、固定 9 種）
  #[garde(dive)]
  pub counters: Counters,
  /// ページ番号のスタイル（前付け＝ローマ数字 / 本文＝算用数字）
  #[garde(dive)]
  pub page_numbering: PageNumbering,
  /// ヘッダー（ページ上端の走り文）のスタイル
  #[garde(dive)]
  pub header: RunningContentStyle,
  /// フッター（ページ下端の走り文）のスタイル
  #[garde(dive)]
  pub footer: RunningContentStyle,
  /// 参考文献セクションのスタイル
  #[garde(dive)]
  pub reference: ReferenceStyle,
  /// ハイパーリンク（hyperref 相当）のスタイル
  #[garde(dive)]
  pub hyperref: HyperrefStyle,
  /// タイトルページ（`\maketitle` 相当）のスタイル
  #[garde(dive)]
  pub title_page: TitlePageStyle,
  /// 目次（table of contents）のスタイル
  #[garde(dive)]
  pub toc: TocStyle,
}

impl Default for Style {
  fn default() -> Self {
    return Self {
      font_size: Length::pt(12.0),
      line_height_factor: 1.2,
      background_color: None,
      heading: HeadingStyles::default(),
      text: TextBlockStyle::default(),
      list: ListStyle::default(),
      table: TableStyle::default(),
      figure: FigureStyle::default(),
      equation: EquationStyle::default(),
      math: MathScriptStyle::default(),
      counters: Counters::default(),
      page_numbering: PageNumbering::default(),
      header: RunningContentStyle::default(),
      footer: RunningContentStyle::default(),
      reference: ReferenceStyle::default(),
      hyperref: HyperrefStyle::default(),
      title_page: TitlePageStyle::default(),
      toc: TocStyle::default(),
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

  /// 指定された名前のカウンタ定義への不変参照を返す（9 種固定のため必ず存在する）。
  #[must_use]
  pub fn counter(&self, name: CounterName) -> &CounterStyle { return self.counters.get(name); }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{Color, HeadingLevel, length::Length};

  use super::Style;
  use crate::counter::CounterName;

  #[test]
  fn validate_accepts_default() {
    assert!(Style::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = Style {
      font_size: Length::pt(0.0),
      ..Default::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    let style = Style {
      line_height_factor: 0.0,
      ..Default::default()
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
    assert_eq!(style.counter(CounterName::Figure).display_name, "Figure");
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
    let mut style = Style::default();
    style.counters.figure.display_name = String::new();
    assert!(style.validate().is_err());
  }

  #[test]
  fn background_color_field_accepts_color_value() {
    let style = Style {
      background_color: Some(Color::new(204, 179, 153)),
      ..Default::default()
    };
    assert!(style.validate().is_ok());
    let color = style.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [204, 179, 153]);
  }
}
