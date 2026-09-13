//! カウンタ（chapter / section / figure 等）のスタイル設定型。
//!
//! `[counters.<name>]` の指定を [`Counters::default`] のカウンタ別既定に重ねて解釈する
//! （見出し・定理と同じ 2 レイヤーマージ）。

use std::{ops::Index, str::FromStr};

use garde::Validate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::style::{CounterTemplate, ReferenceTemplate, number_style::NumberStyle};

/// 固定 9 種のカウンタ定義テーブル（`[counters.<name>]`）。
///
/// TOML からは [`CountersTable`]（各エントリが差分指定 [`CounterStyleOverride`]）として読み、
/// [`Counters::default`] のカウンタ別既定へ重ねて解決済みの値を作る。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(from = "CountersTable")]
pub(crate) struct Counters {
  /// 部
  #[garde(dive)]
  pub part: CounterStyle,
  /// 章
  #[garde(dive)]
  pub chapter: CounterStyle,
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

impl Default for Counters {
  fn default() -> Self {
    return Self {
      part: CounterStyle::new(
        "Part",
        "{n}",
        NumberStyle::RomanUpper,
        "{display_name} {number}",
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
        "{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
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
      section: CounterStyle::new(
        "Section",
        "{chapter}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        &[
          CounterName::Subsection,
          CounterName::Paragraph,
          CounterName::Subparagraph,
        ],
      ),
      subsection: CounterStyle::new(
        "Subsection",
        "{chapter}.{section}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        &[CounterName::Paragraph, CounterName::Subparagraph],
      ),
      paragraph: CounterStyle::new(
        "Paragraph",
        "{chapter}.{section}.{subsection}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        &[CounterName::Subparagraph],
      ),
      subparagraph: CounterStyle::new(
        "Subparagraph",
        "{chapter}.{section}.{subsection}.{paragraph}.{n}",
        NumberStyle::Arabic,
        "{display_name} {number}",
        &[],
      ),
      table: CounterStyle::new("Table", "{chapter}.{n}", NumberStyle::Arabic, "{display_name} {number}", &[]),
      figure: CounterStyle::new("Figure", "{chapter}.{n}", NumberStyle::Arabic, "{display_name} {number}", &[]),
      equation: CounterStyle::new("Equation", "{chapter}.{n}", NumberStyle::Arabic, "({number})", &[]),
    };
  }
}

impl Index<CounterName> for Counters {
  type Output = CounterStyle;

  fn index(&self, name: CounterName) -> &CounterStyle {
    return match name {
      CounterName::Part => &self.part,
      CounterName::Chapter => &self.chapter,
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
#[derive(Debug, Clone, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct CounterStyle {
  /// 表示名（例: `"Figure"`、`"図"`）。`ref_format` の `{display_name}` から参照される
  #[garde(length(chars, min = 1))]
  pub display_name: String,
  /// 番号構築テンプレート。`{n}` で自身、`{<counter_name>}` で他カウンタの値を埋め込む
  ///
  /// 例: `"{n}"`（単独）、`"{chapter}.{n}"`（章番号と連結）、`"第{n}章"`（装飾付き）
  #[garde(dive)]
  pub number_format: CounterTemplate,
  /// 各プレースホルダの数字表記スタイル（参照先カウンタは参照先のスタイルが使われる）
  pub number_style: NumberStyle,
  /// `\ref{label}` の表示テンプレート。`{number}` で `format` の出力、`{display_name}` で
  /// 種別名を埋め込む
  ///
  /// 例: `"{display_name} {number}"` → `"Section 1.2"`、`"({number})"` → `"(1.2)"`
  #[garde(dive)]
  pub ref_format: ReferenceTemplate,
  /// このカウンタが進んだときに 0 にリセットする下位カウンタ群
  pub resets: Vec<CounterName>,
}

impl CounterStyle {
  /// 新しい [`CounterStyle`] を作成するヘルパー
  #[must_use]
  pub(crate) fn new(
    display_name: &str,
    number_format: &str,
    number_style: NumberStyle,
    ref_format: &str,
    resets: &[CounterName],
  ) -> Self {
    return Self {
      display_name: display_name.to_string(),
      number_format: CounterTemplate::parse(number_format),
      number_style,
      ref_format: ReferenceTemplate::parse(ref_format),
      resets: resets.to_vec(),
    };
  }
}

/// `[counters]` テーブル全体の TOML スキーマ。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct CountersTable {
  /// `part` カウンタの上書き
  part: CounterStyleOverride,
  /// `chapter` カウンタの上書き
  chapter: CounterStyleOverride,
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

impl From<CountersTable> for Counters {
  fn from(table: CountersTable) -> Self {
    let mut counters = Self::default();
    table.part.apply(&mut counters.part);
    table.chapter.apply(&mut counters.chapter);
    table.section.apply(&mut counters.section);
    table.subsection.apply(&mut counters.subsection);
    table.paragraph.apply(&mut counters.paragraph);
    table.subparagraph.apply(&mut counters.subparagraph);
    table.table.apply(&mut counters.table);
    table.figure.apply(&mut counters.figure);
    table.equation.apply(&mut counters.equation);
    return counters;
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
  /// 番号構築テンプレート
  number_format: Option<CounterTemplate>,
  /// 数字表記スタイル
  number_style: Option<NumberStyle>,
  /// `\ref{label}` の表示テンプレート
  ref_format: Option<ReferenceTemplate>,
  /// リセットする下位カウンタ群（既定の列を置き換える）
  resets: Option<Vec<CounterName>>,
}

impl CounterStyleOverride {
  /// 自身の `Some` 値で `target` のフィールドを上書きする。
  fn apply(self, target: &mut CounterStyle) {
    if let Some(display_name) = self.display_name {
      target.display_name = display_name;
    }
    if let Some(number_format) = self.number_format {
      target.number_format = number_format;
    }
    if let Some(number_style) = self.number_style {
      target.number_style = number_style;
    }
    if let Some(ref_format) = self.ref_format {
      target.ref_format = ref_format;
    }
    if let Some(resets) = self.resets {
      target.resets = resets;
    }
  }
}

/// カウンタ名（固定 9 種）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CounterName {
  /// 部
  Part,
  /// 章
  Chapter,
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
  /// 固定 9 種のカウンタ名を宣言順（部 → 章 → … → 数式）で並べた配列。
  pub(crate) const ALL: [CounterName; 9] = [
    Self::Part,
    Self::Chapter,
    Self::Section,
    Self::Subsection,
    Self::Paragraph,
    Self::Subparagraph,
    Self::Table,
    Self::Figure,
    Self::Equation,
  ];

  /// `snake_case` の文字列表現を返す（TOML のキーと同じ）
  #[must_use]
  pub(super) fn as_str(self) -> &'static str {
    return match self {
      Self::Part => "part",
      Self::Chapter => "chapter",
      Self::Section => "section",
      Self::Subsection => "subsection",
      Self::Paragraph => "paragraph",
      Self::Subparagraph => "subparagraph",
      Self::Table => "table",
      Self::Figure => "figure",
      Self::Equation => "equation",
    };
  }
}

/// [`CounterName`] の `FromStr` が受理しないカウンタ名を渡されたときのエラー。
#[derive(Debug, Error)]
#[error(
  "カウンタ名は part / chapter / section / subsection / paragraph / subparagraph / table / figure / equation のいずれかである必要があります"
)]
pub(crate) struct ParseCounterNameError;

impl FromStr for CounterName {
  type Err = ParseCounterNameError;

  /// `snake_case` のカウンタ名文字列から [`CounterName`] を復元する
  ///
  /// [`CounterName::as_str`] の走査で実装しているので、両者が食い違うことはない。
  fn from_str(name: &str) -> Result<Self, Self::Err> {
    return Self::ALL.into_iter().find(|c| return c.as_str() == name).ok_or(ParseCounterNameError);
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{CounterName, CounterStyle, Counters, NumberStyle};

  #[test]
  fn validate_accepts_minimal_counter() {
    let counter = CounterStyle::new("Figure", "{chapter}.{n}", NumberStyle::Arabic, "{display_name} {number}", &[]);
    assert!(counter.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_display_name() {
    let counter = CounterStyle::new("", "{n}", NumberStyle::Arabic, "{number}", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_number_format() {
    let counter = CounterStyle::new("Chapter", "", NumberStyle::Arabic, "{number}", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_ref_format() {
    let counter = CounterStyle::new("Chapter", "{n}", NumberStyle::Arabic, "", &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn default_counters_section_references_chapter() {
    let counters = Counters::default();
    assert_eq!(counters.section.number_format.as_str(), "{chapter}.{n}");
    assert_eq!(counters.section.number_style, NumberStyle::Arabic);
    assert_eq!(counters.section.ref_format.as_str(), "{display_name} {number}");
  }

  #[test]
  fn default_counters_equation_uses_parens_ref_format() {
    let counters = Counters::default();
    assert_eq!(counters.equation.ref_format.as_str(), "({number})");
  }

  #[test]
  fn default_counters_part_uses_roman_upper() {
    let counters = Counters::default();
    assert_eq!(counters.part.number_style, NumberStyle::RomanUpper);
  }

  #[test]
  fn partial_entry_keeps_other_defaults() {
    // Arrange — 表示名だけ日本語化する典型例（#561 の再現手順）
    let toml = "
[figure]
display_name = \"図\"
";

    // Act
    let counters: Counters = toml::from_str(toml).unwrap();

    // Assert — 書いたキーだけが変わり、残り 4 キーは figure の既定、他カウンタは無傷
    assert_eq!(counters.figure.display_name, "図");
    assert_eq!(counters.figure.number_format.as_str(), "{chapter}.{n}");
    assert_eq!(counters.figure.number_style, NumberStyle::Arabic);
    assert_eq!(counters.figure.ref_format.as_str(), "{display_name} {number}");
    assert!(counters.figure.resets.is_empty());
    assert_eq!(counters.table.display_name, "Table");
    assert_eq!(counters.chapter.resets.len(), 7);
  }

  #[test]
  fn every_entry_maps_to_its_own_counter() {
    // Arrange — 9 エントリ全部に別々の表示名を与え、`From<CountersTable>` の対応付けを固定する
    let toml = CounterName::ALL
      .into_iter()
      .map(|name| return format!("[{}]\ndisplay_name = \"{}!\"\n", name.as_str(), name.as_str()))
      .collect::<String>();

    // Act
    let counters: Counters = toml::from_str(&toml).unwrap();

    // Assert
    for name in CounterName::ALL {
      assert_eq!(
        counters[name].display_name,
        format!("{}!", name.as_str()),
        "{} の上書きが別のカウンタへ流れている",
        name.as_str()
      );
    }
  }

  #[test]
  fn full_entry_overrides_every_key() {
    // Arrange — 従来どおり 5 キー全部を書いた形
    let toml = "
[figure]
display_name = \"Fig.\"
number_format = \"{section}.{n}\"
number_style = \"roman_lower\"
ref_format = \"{display_name}{number}\"
resets = [\"equation\"]
";

    // Act
    let counters: Counters = toml::from_str(toml).unwrap();

    // Assert
    assert_eq!(counters.figure.display_name, "Fig.");
    assert_eq!(counters.figure.number_format.as_str(), "{section}.{n}");
    assert_eq!(counters.figure.number_style, NumberStyle::RomanLower);
    assert_eq!(counters.figure.ref_format.as_str(), "{display_name}{number}");
    assert_eq!(counters.figure.resets, vec![CounterName::Equation]);
  }

  #[test]
  fn resets_override_replaces_default_list() {
    // Arrange — `resets = []` は既定のリセット列の解除
    let toml = "
[chapter]
resets = []
";

    // Act
    let counters: Counters = toml::from_str(toml).unwrap();

    // Assert
    assert!(counters.chapter.resets.is_empty());
    assert_eq!(counters.chapter.display_name, "Chapter");
  }

  #[test]
  fn empty_table_equals_default() {
    // Arrange
    let parsed: Counters = toml::from_str("").unwrap();

    // Act — `CounterStyle` は `PartialEq` を持たないので直列化した文字列で比べる
    let parsed_text = toml::to_string(&parsed).unwrap();
    let default_text = toml::to_string(&Counters::default()).unwrap();

    // Assert
    assert_eq!(parsed_text, default_text);
  }

  #[test]
  fn serialized_default_roundtrips_through_table() {
    // Arrange — `compiler::test_support::TestProject` が `Style` を `toml::to_string` で書き戻す経路と同じ形
    let text = toml::to_string(&Counters::default()).unwrap();

    // Act
    let reparsed: Counters = toml::from_str(&text).unwrap();

    // Assert
    assert_eq!(toml::to_string(&reparsed).unwrap(), text);
  }

  #[test]
  fn rejects_renamed_format_key() {
    // Arrange — 部分指定でも未知キーは拒否される（P6）
    let toml = "
[figure]
format = \"{chapter}.{n}\"
";

    // Act
    let result: Result<Counters, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "旧キー `format` は未知フィールドとして拒否される");
  }

  #[test]
  fn counters_rejects_unknown_counter_name() {
    // Arrange
    let toml = "
[example]
display_name = \"Example\"
number_format = \"{n}\"
number_style = \"arabic\"
ref_format = \"{number}\"
resets = []
";

    // Act
    let result: Result<Counters, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "未知のカウンタ名 `example` は TOML パース時に拒否される");
  }

  #[test]
  fn counters_rejects_unknown_reset_target() {
    // Arrange
    let toml = "
[chapter]
display_name = \"Chapter\"
number_format = \"{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = [\"example\"]
";

    // Act
    let result: Result<Counters, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "未知の reset 対象 `example` は TOML パース時に拒否される");
  }

  #[test]
  fn counter_name_as_str_matches_snake_case() {
    assert_eq!(CounterName::Part.as_str(), "part");
    assert_eq!(CounterName::Subparagraph.as_str(), "subparagraph");
    assert_eq!(CounterName::Equation.as_str(), "equation");
  }

  #[test]
  fn counters_indexing_returns_matching_field() {
    let counters = Counters::default();
    assert!(std::ptr::eq(&raw const counters[CounterName::Chapter], &raw const counters.chapter));
    assert!(std::ptr::eq(&raw const counters[CounterName::Table], &raw const counters.table));
  }

  #[test]
  fn from_str_roundtrips_as_str_for_all() {
    for counter in CounterName::ALL {
      let name_str = counter.as_str();
      let recovered = name_str.parse::<CounterName>().ok();
      assert_eq!(recovered, Some(counter), "{name_str} から復元できるべき");
    }

    assert!("foo".parse::<CounterName>().is_err());
  }
}
