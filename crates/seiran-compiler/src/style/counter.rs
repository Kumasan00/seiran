//! カウンタ（chapter / section / figure 等）のスタイル設定型。

use std::{ops::Index, str::FromStr};

use garde::Validate;
use serde::Deserialize;
use strum::{IntoStaticStr, VariantArray};
use thiserror::Error;

use crate::{
  document::HeadingLevel,
  project::non_empty_text,
  style::{CounterTemplate, RefNumberTemplate, RefTemplate, number_style::NumberStyle},
};

/// 固定 10 種のカウンタ定義テーブル（`[counters.<name>]`）。
///
/// TOML からは [`CounterStylesTable`]（各エントリが差分指定 [`CounterStyleOverride`]）として読み、
/// [`CounterStyles::default`] のカウンタ別既定へ重ねて解決済みの値を作る。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(from = "CounterStylesTable")]
pub(crate) struct CounterStyles {
  /// 部
  #[garde(dive)]
  pub part: CounterStyle,
  /// 章
  #[garde(dive)]
  pub chapter: CounterStyle,
  /// 付録
  #[garde(dive)]
  pub appendix: CounterStyle,
  /// 節
  #[garde(dive)]
  pub section: CounterStyle,
  /// 小節
  #[garde(dive)]
  pub subsection: CounterStyle,
  /// 段落
  #[garde(dive)]
  pub paragraph: CounterStyle,
  /// 小段落
  #[garde(dive)]
  pub subparagraph: CounterStyle,
  /// 表
  #[garde(dive)]
  pub table: CounterStyle,
  /// 図
  #[garde(dive)]
  pub figure: CounterStyle,
  /// 数式
  #[garde(dive)]
  pub equation: CounterStyle,
}

impl Default for CounterStyles {
  fn default() -> Self {
    return Self {
      part: CounterStyle::new(
        "Part",
        "Parts",
        "{n}",
        NumberStyle::RomanUpper,
        "{display_name} {number}",
        "{number}",
        &[
          CounterName::Chapter,
          CounterName::Section,
          CounterName::Subsection,
          CounterName::Paragraph,
          CounterName::Subparagraph,
        ],
      ),
      chapter: CounterStyle::new(
        "Chapter",
        "Chapters",
        "{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[
          CounterName::Section,
          CounterName::Subsection,
          CounterName::Paragraph,
          CounterName::Subparagraph,
          CounterName::Figure,
          CounterName::Equation,
          CounterName::Table,
        ],
      ),
      appendix: CounterStyle::new(
        "Appendix",
        "Appendices",
        "{n}",
        NumberStyle::AlphaUpper,
        "{display_name} {number}",
        "{number}",
        &[
          CounterName::Chapter,
          CounterName::Section,
          CounterName::Subsection,
          CounterName::Paragraph,
          CounterName::Subparagraph,
          CounterName::Figure,
          CounterName::Equation,
          CounterName::Table,
        ],
      ),
      section: CounterStyle::new(
        "Section",
        "Sections",
        "{appendix}.{chapter}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[
          CounterName::Subsection,
          CounterName::Paragraph,
          CounterName::Subparagraph,
        ],
      ),
      subsection: CounterStyle::new(
        "Subsection",
        "Subsections",
        "{appendix}.{chapter}.{section}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[CounterName::Paragraph, CounterName::Subparagraph],
      ),
      paragraph: CounterStyle::new(
        "Paragraph",
        "Paragraphs",
        "{appendix}.{chapter}.{section}.{subsection}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[CounterName::Subparagraph],
      ),
      subparagraph: CounterStyle::new(
        "Subparagraph",
        "Subparagraphs",
        "{appendix}.{chapter}.{section}.{subsection}.{paragraph}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[],
      ),
      table: CounterStyle::new(
        "Table",
        "Tables",
        "{appendix}.{chapter}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[],
      ),
      figure: CounterStyle::new(
        "Figure",
        "Figures",
        "{appendix}.{chapter}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        "{number}",
        &[],
      ),
      equation: CounterStyle::new(
        "Equation",
        "Equations",
        "{appendix}.{chapter}.{n}",
        NumberStyle::Arabic,
        "{number}",
        "({number})",
        &[],
      ),
    };
  }
}

impl Index<CounterName> for CounterStyles {
  type Output = CounterStyle;

  fn index(&self, name: CounterName) -> &CounterStyle {
    return match name {
      CounterName::Part => &self.part,
      CounterName::Chapter => &self.chapter,
      CounterName::Appendix => &self.appendix,
      CounterName::Section => &self.section,
      CounterName::Subsection => &self.subsection,
      CounterName::Paragraph => &self.paragraph,
      CounterName::Subparagraph => &self.subparagraph,
      CounterName::Table => &self.table,
      CounterName::Figure => &self.figure,
      CounterName::Equation => &self.equation,
    };
  }
}

/// 1 つのカウンタ定義（カウンタ別既定 + `[counters.<name>]` の差分上書きで解決済み）。
///
/// TOML のスキーマは [`CounterStyleOverride`]。
#[derive(Debug, Clone, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct CounterStyle {
  /// 表示名（例: `"Figure"`、`"図"`）。`\ref` の `ref_format` の `{display_name}` に入る
  #[garde(custom(non_empty_text))]
  pub display_name: String,
  /// 複数形の表示名（例: `"Figures"`、`"図"`）。`\refs` / `\refrange` の `ref_format` の `{display_name}` に入る。
  /// `display_name` からは導出しないので、`display_name` だけを上書きしても既定のまま残る
  #[garde(custom(non_empty_text))]
  pub display_name_plural: String,
  /// 番号構築テンプレート。`{n}` で自身、`{<counter_name>}` で採番時点のそのカウンタの値を埋め込む
  /// （`resets` の関係に無いカウンタも書ける）。値 0（未登場）のカウンタは直後の区切りごと省く
  /// （`\chapter` の無い文書で `"{chapter}.{n}"` は `"1"`。規則は `CounterTemplate::expand`）。
  /// `appendix` は既定で `chapter` をリセットするので、`"{appendix}.{chapter}.{n}"` は本文で `"1.1"`、付録で `"A.1"`
  ///
  /// 例: `"{n}"`（単独）、`"{chapter}.{n}"`（章番号と連結）、`"第{n}章"`（装飾付き）
  #[garde(dive)]
  pub number_format: CounterTemplate,
  /// 各プレースホルダの数字表記スタイル（参照先カウンタは参照先のスタイルが使われる）
  pub number_style: NumberStyle,
  /// `\ref{label}` の表示テンプレート。`{number}` は必須で、`ref_number_format` を通した番号
  /// （`\refs` / `\refrange` では一覧・範囲）が入る。`{display_name}` で種別名を埋め込む
  ///
  /// 例: `"{display_name} {number}"` → `"Section 1.2"`、`"{number}"` と `ref_number_format = "({number})"` → `"(1.2)"`
  #[garde(dive)]
  pub ref_format: RefTemplate,
  /// 参照 1 件ぶんの番号の見せ方。`{number}`（必須）に `number_format` の出力が入り、
  /// 結果が `ref_format` の `{number}` へ入る
  ///
  /// 例: `"{number}"`（そのまま）、`"({number})"`（括弧付き。equation の既定）
  #[garde(dive)]
  pub ref_number_format: RefNumberTemplate,
  /// このカウンタが進んだときに 0 にリセットする下位カウンタ群
  pub resets: Vec<CounterName>,
}

impl CounterStyle {
  /// 新しい [`CounterStyle`] を作成するヘルパー
  #[must_use]
  fn new(
    display_name: &str,
    display_name_plural: &str,
    number_format: &str,
    number_style: NumberStyle,
    ref_format: &str,
    ref_number_format: &str,
    resets: &[CounterName],
  ) -> Self {
    return Self {
      display_name: display_name.to_string(),
      display_name_plural: display_name_plural.to_string(),
      number_format: CounterTemplate::parse(number_format),
      number_style,
      ref_format: RefTemplate::parse(ref_format),
      ref_number_format: RefNumberTemplate::parse(ref_number_format),
      resets: resets.to_vec(),
    };
  }
}

/// `[counters]` テーブル全体の TOML スキーマ。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct CounterStylesTable {
  /// `part` カウンタの上書き
  part: CounterStyleOverride,
  /// `chapter` カウンタの上書き
  chapter: CounterStyleOverride,
  /// `appendix` カウンタの上書き
  appendix: CounterStyleOverride,
  /// `section` カウンタの上書き
  section: CounterStyleOverride,
  /// `subsection` カウンタの上書き
  subsection: CounterStyleOverride,
  /// `paragraph` カウンタの上書き
  paragraph: CounterStyleOverride,
  /// `subparagraph` カウンタの上書き
  subparagraph: CounterStyleOverride,
  /// `table` カウンタの上書き
  table: CounterStyleOverride,
  /// `figure` カウンタの上書き
  figure: CounterStyleOverride,
  /// `equation` カウンタの上書き
  equation: CounterStyleOverride,
}

impl From<CounterStylesTable> for CounterStyles {
  fn from(table: CounterStylesTable) -> Self {
    let defaults = Self::default();
    return Self {
      part: table.part.apply(defaults.part),
      chapter: table.chapter.apply(defaults.chapter),
      appendix: table.appendix.apply(defaults.appendix),
      section: table.section.apply(defaults.section),
      subsection: table.subsection.apply(defaults.subsection),
      paragraph: table.paragraph.apply(defaults.paragraph),
      subparagraph: table.subparagraph.apply(defaults.subparagraph),
      table: table.table.apply(defaults.table),
      figure: table.figure.apply(defaults.figure),
      equation: table.equation.apply(defaults.equation),
    };
  }
}

/// [`CounterStyle`] の各フィールドを `Option<_>` で覆った差分指定型（`[counters.<name>]` の TOML スキーマ）。
///
/// `None` のフィールドはカウンタ別既定のまま残す。`resets` は `Some` なら既定のリセット列を
/// 丸ごと置き換える（`resets = []` で既定のリセットを解除できる）。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct CounterStyleOverride {
  /// 表示名
  display_name: Option<String>,
  /// 複数形の表示名
  display_name_plural: Option<String>,
  /// 番号構築テンプレート
  number_format: Option<CounterTemplate>,
  /// 数字表記スタイル
  number_style: Option<NumberStyle>,
  /// `\ref{label}` の表示テンプレート
  ref_format: Option<RefTemplate>,
  /// 参照 1 件ぶんの番号の書式
  ref_number_format: Option<RefNumberTemplate>,
  /// リセットする下位カウンタ群（既定の列を置き換える）
  resets: Option<Vec<CounterName>>,
}

impl CounterStyleOverride {
  /// 自身の `Some` 値で `base` のフィールドを置き換えた値を返す。
  ///
  /// `self` の分割と戻り値のリテラルがどちらも `..` 無しなので、差分指定型・解決済み型のどちらに
  /// フィールドを足しても、ここで扱いを決めるまでコンパイルが通らない。
  fn apply(self, base: CounterStyle) -> CounterStyle {
    let Self {
      display_name,
      display_name_plural,
      number_format,
      number_style,
      ref_format,
      ref_number_format,
      resets,
    } = self;
    return CounterStyle {
      display_name: display_name.unwrap_or(base.display_name),
      display_name_plural: display_name_plural.unwrap_or(base.display_name_plural),
      number_format: number_format.unwrap_or(base.number_format),
      number_style: number_style.unwrap_or(base.number_style),
      ref_format: ref_format.unwrap_or(base.ref_format),
      ref_number_format: ref_number_format.unwrap_or(base.ref_number_format),
      resets: resets.unwrap_or(base.resets),
    };
  }
}

/// カウンタ名（固定 10 種）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, IntoStaticStr, VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub(crate) enum CounterName {
  /// 部
  Part,
  /// 章
  Chapter,
  /// 付録
  Appendix,
  /// 節
  Section,
  /// 小節
  Subsection,
  /// 段落
  Paragraph,
  /// 小段落
  Subparagraph,
  /// 表
  Table,
  /// 図
  Figure,
  /// 数式
  Equation,
}

impl CounterName {
  /// 見出しレベルの採番に使う見出しカウンタを返す（全見出しレベルが固定 10 種のうち同名の 1 つに対応する）
  #[must_use]
  pub(crate) fn for_heading(level: HeadingLevel) -> Self {
    return match level {
      HeadingLevel::Part => Self::Part,
      HeadingLevel::Chapter => Self::Chapter,
      HeadingLevel::Appendix => Self::Appendix,
      HeadingLevel::Section => Self::Section,
      HeadingLevel::Subsection => Self::Subsection,
      HeadingLevel::Paragraph => Self::Paragraph,
      HeadingLevel::Subparagraph => Self::Subparagraph,
    };
  }
}

/// [`CounterName`] の `FromStr` が受理しないカウンタ名を渡されたときのエラー。
#[derive(Debug, Error)]
#[error(
  "カウンタ名は part / chapter / appendix / section / subsection / paragraph / subparagraph / table / figure / equation のいずれかである必要があります"
)]
pub(crate) struct ParseCounterNameError;

impl FromStr for CounterName {
  type Err = ParseCounterNameError;

  /// `snake_case` のカウンタ名文字列から [`CounterName`] を復元する
  ///
  /// `IntoStaticStr` の derive が出す綴りと全 variant を照合して実装しているので、両者が食い違うことはない。
  fn from_str(name: &str) -> Result<Self, Self::Err> {
    return Self::VARIANTS
      .iter()
      .copied()
      .find(|&c| return <&str>::from(c) == name)
      .ok_or(ParseCounterNameError);
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use strum::VariantArray;

  use super::{CounterName, CounterStyle, CounterStyles, NumberStyle};
  use crate::document::HeadingLevel;

  #[test]
  fn validate_rejects_empty_display_name() {
    let counter = CounterStyle::new("", "Chapters", "{n}", NumberStyle::Arabic, "{number}", "{number}", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_display_name_plural() {
    let counter = CounterStyle::new("Chapter", "", "{n}", NumberStyle::Arabic, "{number}", "{number}", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn default_counters_have_plural_display_names() {
    let expected = [
      (CounterName::Part, "Parts"),
      (CounterName::Chapter, "Chapters"),
      (CounterName::Appendix, "Appendices"),
      (CounterName::Section, "Sections"),
      (CounterName::Subsection, "Subsections"),
      (CounterName::Paragraph, "Paragraphs"),
      (CounterName::Subparagraph, "Subparagraphs"),
      (CounterName::Table, "Tables"),
      (CounterName::Figure, "Figures"),
      (CounterName::Equation, "Equations"),
    ];
    let counters = CounterStyles::default();

    for (name, plural) in expected {
      assert_eq!(counters[name].display_name_plural, plural, "{name:?}");
    }
  }

  #[test]
  fn display_name_plural_override_keeps_other_defaults() {
    let counters: CounterStyles = toml::from_str("[figure]\ndisplay_name_plural = \"図\"\n").unwrap();

    assert_eq!(counters.figure.display_name_plural, "図");
    assert_eq!(counters.figure.display_name, "Figure", "単数形は書かれなければ既定のまま");
    assert_eq!(counters.table.display_name_plural, "Tables");
  }

  #[test]
  fn validate_rejects_empty_ref_format() {
    let counter = CounterStyle::new("Chapter", "Chapters", "{n}", NumberStyle::Arabic, "", "{number}", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn default_counters_section_references_chapter() {
    let counters = CounterStyles::default();
    assert_eq!(counters.section.number_format.as_str(), "{appendix}.{chapter}.{n}");
    assert_eq!(counters.section.number_style, NumberStyle::Arabic);
    assert_eq!(counters.section.ref_format.as_str(), "{display_name} {number}");
  }

  #[test]
  fn validate_rejects_ref_format_without_number() {
    let counter =
      CounterStyle::new("Chapter", "Chapters", "{n}", NumberStyle::Arabic, "{display_name}", "{number}", &[]);
    assert!(counter.validate().is_err());

    let counter = CounterStyle::new("Chapter", "Chapters", "{n}", NumberStyle::Arabic, "{number}", "()", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn default_counters_equation_wraps_each_number() {
    let counters = CounterStyles::default();
    assert_eq!(counters.equation.ref_format.as_str(), "{number}");
    assert_eq!(counters.equation.ref_number_format.as_str(), "({number})");
    assert_eq!(counters.figure.ref_number_format.as_str(), "{number}");

    let overridden: CounterStyles = toml::from_str("[equation]\nref_number_format = \"[{number}]\"\n").unwrap();
    assert_eq!(overridden.equation.ref_number_format.as_str(), "[{number}]");
  }

  #[test]
  fn default_appendix_counter() {
    let counters = CounterStyles::default();

    let appendix = &counters.appendix;
    assert_eq!(appendix.display_name, "Appendix");
    assert_eq!(appendix.number_format.as_str(), "{n}");
    assert_eq!(appendix.number_style, NumberStyle::AlphaUpper);
    assert_eq!(appendix.ref_format.as_str(), "{display_name} {number}");
    assert_eq!(appendix.ref_number_format.as_str(), "{number}");
    // 章が進んだときに戻るものに加えて章自身も戻す（付録の下では章番号が 0 になる）
    assert!(appendix.resets.contains(&CounterName::Chapter));
    for reset in &counters.chapter.resets {
      assert!(appendix.resets.contains(reset), "{reset:?}");
    }
    assert_eq!(appendix.resets.len(), counters.chapter.resets.len() + 1);
  }

  #[test]
  fn appendix_counter_override_keeps_other_defaults() {
    let counters: CounterStyles = toml::from_str("[appendix]\ndisplay_name = \"付録\"\n").unwrap();

    assert_eq!(counters.appendix.display_name, "付録");
    assert_eq!(counters.appendix.number_style, NumberStyle::AlphaUpper);
  }

  #[test]
  fn appendix_counter_rejects_unknown_key() {
    let result: Result<CounterStyles, _> = toml::from_str("[appendix]\nkind = \"chapter\"\n");

    assert!(result.is_err());
  }

  #[test]
  fn default_counters_part_uses_roman_upper() {
    let counters = CounterStyles::default();
    assert_eq!(counters.part.number_style, NumberStyle::RomanUpper);
  }

  #[test]
  fn partial_entry_keeps_other_defaults() {
    let toml = "
[figure]
display_name = \"図\"
";

    let counters: CounterStyles = toml::from_str(toml).unwrap();

    assert_eq!(counters.figure.display_name, "図");
    assert_eq!(counters.figure.number_format.as_str(), "{appendix}.{chapter}.{n}");
    assert_eq!(counters.figure.number_style, NumberStyle::Arabic);
    assert_eq!(counters.figure.ref_format.as_str(), "{display_name} {number}");
    assert_eq!(counters.figure.resets, []);
    assert_eq!(counters.table.display_name, "Table");
    assert_eq!(counters.chapter.resets.len(), 7);
  }

  #[test]
  fn every_entry_maps_to_its_own_counter() {
    // 10 エントリ全部に別々の表示名を与え、`From<CounterStylesTable>` の対応付けを固定する
    let toml = CounterName::VARIANTS
      .iter()
      .map(|&name| {
        let key: &str = name.into();
        return format!("[{key}]\ndisplay_name = \"{key}!\"\n");
      })
      .collect::<String>();

    let counters: CounterStyles = toml::from_str(&toml).unwrap();

    for &name in CounterName::VARIANTS {
      let key: &str = name.into();
      assert_eq!(counters[name].display_name, format!("{key}!"), "{key} の上書きが別のカウンタへ流れている");
    }
  }

  #[test]
  fn full_entry_overrides_every_key() {
    let toml = "
[figure]
display_name = \"Fig.\"
display_name_plural = \"Figs.\"
number_format = \"{section}.{n}\"
number_style = \"roman_lower\"
ref_format = \"{display_name}{number}\"
ref_number_format = \"[{number}]\"
resets = [\"equation\"]
";

    let counters: CounterStyles = toml::from_str(toml).unwrap();

    assert_eq!(counters.figure.display_name, "Fig.");
    assert_eq!(counters.figure.display_name_plural, "Figs.");
    assert_eq!(counters.figure.number_format.as_str(), "{section}.{n}");
    assert_eq!(counters.figure.number_style, NumberStyle::RomanLower);
    assert_eq!(counters.figure.ref_format.as_str(), "{display_name}{number}");
    assert_eq!(counters.figure.ref_number_format.as_str(), "[{number}]");
    assert_eq!(counters.figure.resets, vec![CounterName::Equation]);
  }

  #[test]
  fn resets_override_replaces_default_list() {
    let toml = "
[chapter]
resets = []
";
    let counters: CounterStyles = toml::from_str(toml).unwrap();
    assert_eq!(counters.chapter.resets, []);
    assert_eq!(counters.chapter.display_name, "Chapter");
  }

  #[test]
  fn empty_table_equals_default() {
    let parsed: CounterStyles = toml::from_str("").unwrap();
    // `CounterStyle` は `PartialEq` を持たないので全フィールドを出す `Debug` 表現で比べる
    let parsed_text = format!("{parsed:?}");
    let default_text = format!("{:?}", CounterStyles::default());
    assert_eq!(parsed_text, default_text);
  }

  #[test]
  fn rejects_renamed_format_key() {
    let toml = "
[figure]
format = \"{chapter}.{n}\"
";
    let result: Result<CounterStyles, _> = toml::from_str(toml);
    assert!(result.is_err(), "旧キー `format` は未知フィールドとして拒否される");
  }

  #[test]
  fn counters_rejects_unknown_counter_name() {
    let toml = "
[example]
display_name = \"Example\"
number_format = \"{n}\"
number_style = \"arabic\"
ref_format = \"{number}\"
resets = []
";
    let result: Result<CounterStyles, _> = toml::from_str(toml);
    assert!(result.is_err(), "未知のカウンタ名 `example` は TOML パース時に拒否される");
  }

  #[test]
  fn counters_rejects_unknown_reset_target() {
    let toml = "
[chapter]
display_name = \"Chapter\"
number_format = \"{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = [\"example\"]
";
    let result: Result<CounterStyles, _> = toml::from_str(toml);
    assert!(result.is_err(), "未知の reset 対象 `example` は TOML パース時に拒否される");
  }

  #[test]
  fn serde_accepts_strum_spelling_for_all() {
    // serde の `rename_all` と strum の `serialize_all` は別の derive 属性なので、綴りの一致をここで固定する
    for &counter in CounterName::VARIANTS {
      let key: &str = counter.into();
      let parsed: CounterName = toml::Value::String(key.to_owned()).try_into().unwrap();
      assert_eq!(parsed, counter);
    }
  }

  #[test]
  fn from_str_roundtrips_strum_spelling_for_all() {
    for &counter in CounterName::VARIANTS {
      let name_str: &str = counter.into();
      let recovered = name_str.parse::<CounterName>().ok();
      assert_eq!(recovered, Some(counter), "{name_str} から復元できるべき");
    }

    assert!("foo".parse::<CounterName>().is_err());
  }

  #[test]
  fn for_heading_maps_each_level() {
    assert_eq!(CounterName::for_heading(HeadingLevel::Part), CounterName::Part);
    assert_eq!(CounterName::for_heading(HeadingLevel::Chapter), CounterName::Chapter);
    assert_eq!(CounterName::for_heading(HeadingLevel::Subparagraph), CounterName::Subparagraph);
  }
}
