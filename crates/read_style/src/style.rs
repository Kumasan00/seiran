//! スタイル設定全体を保持する [`Style`] 構造体。
//!
//! `lowering` / `pdf_gen` から実際に参照される本文・見出し・リスト・数式・図表・目次など、
//! 描画パイプラインに直接効くフィールドをトップレベルに集約する。サブスタイル型
//! （[`crate::caption::CaptionStyle`] 等）はクレート直下の各モジュールに置く。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{Color, HeadingLevel};

use crate::{
  counter::{CounterName, CounterStyle, Counters},
  equation::EquationStyle,
  figure::FigureStyle,
  heading::{HeadingStyle, HeadingStyles},
  hyperref::HyperrefStyle,
  list::ListStyle,
  math::{MathBlockStyle, MathScriptStyle},
  page_numbering::PageNumbering,
  quote::QuoteStyle,
  reference::ReferenceStyle,
  running::RunningContentStyle,
  table::TableStyle,
  text::TextBlockStyle,
  theorem::{TheoremClass, TheoremStyle, Theorems},
  title_page::TitlePageStyle,
  toc::TocStyle,
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
///
/// TOML キーはそのままトップレベル（`[text]` / `[heading.section]` / `[figure.caption]` 等）に出る。
/// 本文テキストの既定見た目（フォントサイズ・行高など）は `[text]`（[`TextBlockStyle`]）に集約する。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct Style {
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
  /// 引用ブロック（quote / quotation）のスタイル
  #[garde(dive)]
  pub quote: QuoteStyle,
  /// 表のスタイル
  #[garde(dive)]
  pub table: TableStyle,
  /// 図フロートのスタイル
  #[garde(dive)]
  pub figure: FigureStyle,
  /// ディスプレイ数式のスタイル
  #[garde(dive)]
  pub equation: EquationStyle,
  /// 数式レイアウト（上付き / 下付き）のスタイル
  #[garde(dive)]
  pub math: MathScriptStyle,
  /// 複数行ディスプレイ数式環境（align / gather / cases / matrix）のレイアウトスタイル
  #[garde(dive)]
  pub math_block: MathBlockStyle,
  /// カウンタ定義テーブル（`[counters.<name>]`、固定 9 種）
  #[garde(dive)]
  pub counters: Counters,
  /// 定理クラス定義テーブル（`[theorems.<class>]`、固定 10 種）
  ///
  /// クラス別の既定値は [`Theorems::default`] が供給する。TOML で `[theorems.lemma]` のように
  /// 一部だけ書いた場合、欠落フィールドは [`crate::theorem::default_for_class`] で埋まる。
  /// `heading` と同様 `#[serde(from = ...)]` で構築するため検証は `validate_values` で個別に行う。
  #[garde(skip)]
  pub theorems: Theorems,
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
      background_color: None,
      heading: HeadingStyles::default(),
      text: TextBlockStyle::default(),
      list: ListStyle::default(),
      quote: QuoteStyle::default(),
      table: TableStyle::default(),
      figure: FigureStyle::default(),
      equation: EquationStyle::default(),
      math: MathScriptStyle::default(),
      math_block: MathBlockStyle::default(),
      counters: Counters::default(),
      theorems: Theorems::default(),
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

  /// 指定された定理クラスのスタイル定義への不変参照を返す（10 種固定のため必ず存在する）。
  #[must_use]
  pub fn theorem(&self, class: TheoremClass) -> &TheoremStyle { return self.theorems.get(class); }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{Color, HeadingLevel, length::Length};

  use super::Style;
  use crate::{counter::CounterName, theorem::TheoremClass};

  #[test]
  fn validate_accepts_default() {
    assert!(Style::default().validate().is_ok());
  }

  #[test]
  fn validate_dives_into_text_font_size() {
    // 本文フォントサイズは `[text]` に移動した（#124）。`#[garde(dive)]` で Style 検証が拾う。
    let mut style = Style::default();
    style.text.font_size = Length::pt(0.0);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_text_line_height_factor() {
    let mut style = Style::default();
    style.text.line_height_factor = 0.0;
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
  fn validate_dives_into_quote_indent() {
    let mut style = Style::default();
    style.quote.indent = Length::pt(-0.1);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_counters_display_name() {
    let mut style = Style::default();
    style.counters.figure.display_name = String::new();
    assert!(style.validate().is_err());
  }

  #[test]
  fn theorem_accessor_finds_proof() {
    let style = Style::default();
    assert_eq!(style.theorem(TheoremClass::Proof).display_name, "Proof");
    assert!(style.theorem(TheoremClass::Proof).unnumbered);
  }

  #[test]
  fn validate_detects_invalid_theorem_top_margin() {
    // Style::validate() は #[garde(skip)] のため theorems を見ない。検証は validate_values で行われる。
    let mut style = Style::default();
    style.theorems.theorem.style.top_margin = Length::pt(-0.1);
    // 直接 TheoremStyle を検証して不正が検出されることを確認する
    assert!(style.theorems.theorem.validate().is_err());
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

  #[test]
  fn rejects_renamed_top_level_text_keys() {
    // 旧トップレベルキー `font_size` / `line_height_factor` は `[text]` へ移動した（#124）。
    // `deny_unknown_fields` が未知フィールドとして弾く（移行: 旧 config は即エラー）。
    assert!(toml::from_str::<Style>("font_size = \"12pt\"\n").is_err());
    assert!(toml::from_str::<Style>("line_height_factor = 1.2\n").is_err());
  }
}
