//! コアスタイル設定（[`CoreStyle`]）。
//!
//! `lowering` / `pdf_gen` から実際に参照される本文・見出し・リスト・数式・図表など、
//! 描画パイプラインに直接効くフィールドを集約する。未実装領域（脚注・目次・参考文献等）
//! は [`crate::extended::ExtendedStyle`] に分離している。
//!
//! TOML 上では [`crate::Style`] が `#[serde(flatten)]` で展開するため、`CoreStyle` の
//! フィールドは `[heading.section]` / `[figure.caption]` のようにトップレベルに現れる。

pub mod caption;
pub mod counter;
pub mod equation;
pub mod figure;
pub mod heading;
pub mod list;
pub mod math;
pub mod reference;
pub mod running;
pub mod table;
pub mod text;

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{
  Color,
  length::{Length, positive},
};

use crate::core::{
  counter::Counters, equation::EquationStyle, figure::FigureStyle, heading::HeadingStyles, list::ListStyle,
  math::MathScriptStyle, reference::ReferenceStyle, running::RunningContentStyle, table::TableStyle,
  text::TextBlockStyle,
};

/// コアスタイル設定。`lowering` / `pdf_gen` から参照されるフィールドの集合。
///
/// [`crate::Style`] からは `#[serde(flatten)]` で展開されるため、TOML キーは
/// 従来どおりトップレベル（`font_size` / `[heading.section]` / `[figure.caption]` 等）に出る。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct CoreStyle {
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
  /// 一部だけ書いた場合、欠落レベルは [`heading::default_for_level`] で埋まる。
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
  /// ヘッダー（ページ上端の走り文）のスタイル
  #[garde(dive)]
  pub header: RunningContentStyle,
  /// フッター（ページ下端の走り文）のスタイル
  #[garde(dive)]
  pub footer: RunningContentStyle,
  /// 参考文献セクションのスタイル
  #[garde(dive)]
  pub reference: ReferenceStyle,
}

impl Default for CoreStyle {
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
      header: RunningContentStyle::default(),
      footer: RunningContentStyle::default(),
      reference: ReferenceStyle::default(),
    };
  }
}
