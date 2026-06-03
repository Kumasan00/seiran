//! スタイル設定全体を保持する [`Style`] 構造体と既定値の定義。

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  CounterStyle, EquationStyle, FigureStyle, FootnoteStyle, HeadingStyle, HyperrefStyle, ReferenceStyle, TableStyle,
  TocStyle, counter::default_counters,
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct Style {
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 本文テキスト色 RGB（各成分 0-255、オプション）。未指定時はデフォルト（黒）。
  #[garde(skip)]
  pub text_color: Option<[u8; 3]>,
  /// 背景色 RGB（各成分 0-255、オプション）。未指定時は背景色なし。
  #[garde(skip)]
  pub background_color: Option<[u8; 3]>,
  #[garde(dive)]
  pub part: HeadingStyle,
  #[garde(dive)]
  pub chapter: HeadingStyle,
  #[garde(dive)]
  pub section: HeadingStyle,
  #[garde(dive)]
  pub subsection: HeadingStyle,
  #[garde(dive)]
  pub paragraph: HeadingStyle,
  #[garde(dive)]
  pub subparagraph: HeadingStyle,
  #[garde(dive)]
  pub figure: FigureStyle,
  #[garde(dive)]
  pub equation: EquationStyle,
  #[garde(dive)]
  pub table: TableStyle,
  #[garde(dive)]
  pub footnote: FootnoteStyle,
  #[garde(dive)]
  pub toc: TocStyle,
  #[garde(dive)]
  pub hyperref: HyperrefStyle,
  /// カウンタ定義テーブル（`[counters.<name>]`）。
  /// `parser::evaluator::counter::CounterRegistry::from_style` が読み取って実行時表現に変換する。
  #[garde(dive)]
  pub counters: HashMap<String, CounterStyle>,
  #[garde(dive)]
  pub reference: ReferenceStyle,
}

impl Default for Style {
  fn default() -> Self {
    // 軸補 i18n: デフォルトは英語。日本語化したい場合は style.toml で上書きする。
    // プレースホルダは `{number}` `{title}` のみ:
    //   - `{number}` は HeadingNumber::dotted() の値（Part: "1"、Chapter: "1"、Section: "1.2"、…）
    //   - `{title}` は見出しタイトルのプレーンテキスト
    return Self {
      font_size: 12.0,
      line_height_factor: 1.2,
      text_color: None,
      background_color: None,
      part: HeadingStyle {
        format: "Part {number}: {title}".to_string(),
        font_size: 40.0,
        bottom_margin: 20.0,
        page_break_before: true,
        page_break_after: true,
      },
      chapter: HeadingStyle {
        format: "Chapter {number}: {title}".to_string(),
        font_size: 25.0,
        bottom_margin: 15.0,
        page_break_before: true,
        page_break_after: false,
      },
      section: HeadingStyle {
        format: "{number} {title}".to_string(),
        font_size: 20.0,
        bottom_margin: 10.0,
        page_break_before: false,
        page_break_after: false,
      },
      subsection: HeadingStyle {
        format: "{number} {title}".to_string(),
        font_size: 16.0,
        bottom_margin: 10.0,
        page_break_before: false,
        page_break_after: false,
      },
      paragraph: HeadingStyle {
        format: "{number} {title}".to_string(),
        font_size: 14.0,
        bottom_margin: 5.0,
        page_break_before: false,
        page_break_after: false,
      },
      subparagraph: HeadingStyle {
        format: "{number} {title}".to_string(),
        font_size: 12.0,
        bottom_margin: 5.0,
        page_break_before: false,
        page_break_after: false,
      },
      figure: FigureStyle::default(),
      equation: EquationStyle::default(),
      table: TableStyle::default(),
      footnote: FootnoteStyle::default(),
      toc: TocStyle::default(),
      hyperref: HyperrefStyle::default(),
      counters: default_counters(),
      reference: ReferenceStyle::default(),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::Style;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(Style::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = Style {
      font_size: 0.0,
      ..Style::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_font_size() {
    // Arrange
    let style = Style {
      font_size: -1.0,
      ..Style::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    // Arrange
    let style = Style {
      line_height_factor: 0.0,
      ..Style::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_line_height_factor() {
    // Arrange
    let style = Style {
      line_height_factor: -1.0,
      ..Style::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_heading() {
    // Arrange: dive で chapter 内の不正値が伝播することを確認
    let mut style = Style::default();
    style.chapter.bottom_margin = -1.0;

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_reference() {
    // Arrange: dive で reference 内の不正値が伝播することを確認
    let mut style = Style::default();
    style.reference.format = String::new();

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_figure() {
    // Arrange
    let mut style = Style::default();
    style.figure.caption_format = String::new();

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_equation() {
    // Arrange
    let mut style = Style::default();
    style.equation.top_margin = -1.0;

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_table() {
    // Arrange
    let mut style = Style::default();
    style.table.rule_thickness = -0.1;

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_footnote() {
    // Arrange
    let mut style = Style::default();
    style.footnote.marker_format = String::new();

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_toc() {
    // Arrange
    let mut style = Style::default();
    style.toc.max_depth = 0;

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_counters() {
    // Arrange: HashMap の中身もバリデーションされること
    let mut style = Style::default();
    if let Some(counter) = style.counters.get_mut("figure") {
      counter.display_name = String::new();
    }

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
