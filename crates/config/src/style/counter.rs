//! カウンタ（chapter / section / figure 等）のスタイル設定型。
//!
//! TOML 上では `[counters.<name>]` のキーに [`CounterStyle`] の各フィールド
//! （`display_name` / `number_format` / `number_style` / `ref_format` / `resets`）を書く。
//! `<name>` は固定 9 種（[`CounterName`]）のみが許可され、それ以外のキーは TOML
//! パース時に拒否される。
//!
//! ## `number_format` テンプレート（カウンタ番号の構築）
//!
//! `number_format` はテンプレート文字列で、`{n}` がそのカウンタ自身の値、`{<counter_name>}` が
//! 他カウンタの値を表す。各プレースホルダは参照先カウンタの [`NumberStyle`] に従って
//! レンダリングされる（再帰展開はしない）。番号を「構築」する書式であり、`\ref` に出す
//! [`CounterStyle::ref_format`] とは役割が異なる（number = 構築 / ref = 参照）。
//!
//! - 例: chapter の `number_format = "{n}"` → `"1"`、`"2"`
//! - 例: section の `number_format = "{chapter}.{n}"` → `"1.1"`、`"1.2"`
//! - 例: 部の `number_format = "{n}"` + `number_style = "roman_upper"` → `"I"`、`"II"`
//!
//! ## `ref_format` テンプレート（`\ref` の表示）
//!
//! `\ref{label}` が返す文字列の整形に使うテンプレート。`{number}` が `number_format` の出力（裸の
//! 番号）、`{display_name}` が `display_name` フィールドの値を指す。
//!
//! - 例: section の `ref_format = "{display_name} {number}"` → `"Section 1.2"`
//! - 例: equation の `ref_format = "({number})"` → `"(1.2)"`
//! - 例: 日本語化したい場合は `display_name = "図"` + `ref_format = "{display_name} {number}"` → `"図 1.2"`
//!
//! equation の `ref_format` は `\ref{eq:x}` の表示専用で、式の横に出る番号の体裁を決める
//! [`crate::style::EquationStyle::number_format`]（`[equation]`）とは別物。既定値がどちらも `"({number})"`
//! で一致するのは LaTeX 慣習で「式の横」も「素の相互参照」も括弧付き番号だからで、両者は独立に
//! 変更できる（`EquationStyle::number_format` は `figure` の `caption.format` に相当する数式ブロックの体裁）。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::style::number_style::NumberStyle;

/// 固定 9 種のカウンタ定義テーブル（`[counters.<name>]`）
///
/// TOML では各カウンタが独立したサブテーブルとして現れる。`#[serde(default)]` により
/// 未指定フィールドは [`Counters::default`] の値で埋まり、`deny_unknown_fields` により
/// 9 種以外のキー（例: `[counters.example]`）は TOML パース時に拒否される。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct Counters {
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

impl Counters {
  /// 指定したカウンタ名の定義への不変参照を返す（9 種固定のため必ず存在する）
  #[must_use]
  pub fn get(&self, name: CounterName) -> &CounterStyle {
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

  /// `(CounterName, &CounterStyle)` の組を 9 個まとめて返すイテレータ
  pub fn iter(&self) -> impl Iterator<Item = (CounterName, &CounterStyle)> {
    return [
      (CounterName::Part, &self.part),
      (CounterName::Chapter, &self.chapter),
      (CounterName::Section, &self.section),
      (CounterName::Subsection, &self.subsection),
      (CounterName::Paragraph, &self.paragraph),
      (CounterName::Subparagraph, &self.subparagraph),
      (CounterName::Table, &self.table),
      (CounterName::Figure, &self.figure),
      (CounterName::Equation, &self.equation),
    ]
    .into_iter();
  }
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

/// 1 つのカウンタ定義（TOML スキーマ）
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct CounterStyle {
  /// 表示名（例: `"Figure"`、`"図"`）。`ref_format` の `{display_name}` から参照される
  #[garde(length(chars, min = 1))]
  pub display_name: String,
  /// 番号構築テンプレート。`{n}` で自身、`{<counter_name>}` で他カウンタの値を埋め込む
  ///
  /// 例: `"{n}"`（単独）、`"{chapter}.{n}"`（章番号と連結）、`"第{n}章"`（装飾付き）
  #[garde(length(chars, min = 1), custom(crate::style::placeholder::counter_format))]
  pub number_format: String,
  /// 各プレースホルダの数字表記スタイル（参照先カウンタは参照先のスタイルが使われる）
  pub number_style: NumberStyle,
  /// `\ref{label}` の表示テンプレート。`{number}` で `format` の出力、`{display_name}` で
  /// 種別名を埋め込む
  ///
  /// 例: `"{display_name} {number}"` → `"Section 1.2"`、`"({number})"` → `"(1.2)"`
  #[garde(length(chars, min = 1), custom(crate::style::placeholder::ref_format))]
  pub ref_format: String,
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
      number_format: number_format.to_string(),
      number_style,
      ref_format: ref_format.to_string(),
      resets: resets.to_vec(),
    };
  }
}

/// カウンタ名（固定 9 種）。TOML 上の `[counters.<name>]` キーおよび `resets` 配列の要素として
/// 使われ、未知の名前は TOML パース時に弾かれる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterName {
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
  ///
  /// プレースホルダ検証（`crate::style::placeholder`）が `{<counter_name>}` 参照の許可セットを
  /// 構築する際の単一ソースとして使う。
  pub const ALL: [CounterName; 9] = [
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
  pub fn as_str(self) -> &'static str {
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

  /// `snake_case` のカウンタ名文字列から [`CounterName`] を復元する
  ///
  /// 固定 9 種の範囲内の名前のみを認識し、該当しない文字列は `None` を返す。
  /// テンプレート内の `{chapter}` のような自由記述プレースホルダから enum に戻すために使う。
  #[must_use]
  pub fn from_name(name: &str) -> Option<Self> { return Self::ALL.into_iter().find(|c| return c.as_str() == name); }
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
  fn default_counters_contains_expected_names() {
    let counters = Counters::default();
    let names: Vec<CounterName> = counters.iter().map(|(n, _)| return n).collect();
    for expected in [
      CounterName::Part,
      CounterName::Chapter,
      CounterName::Section,
      CounterName::Subsection,
      CounterName::Paragraph,
      CounterName::Subparagraph,
      CounterName::Figure,
      CounterName::Equation,
      CounterName::Table,
    ] {
      assert!(names.contains(&expected), "missing counter: {}", expected.as_str());
    }
  }

  #[test]
  fn default_counters_section_references_chapter() {
    let counters = Counters::default();
    assert_eq!(counters.section.number_format, "{chapter}.{n}");
    assert_eq!(counters.section.number_style, NumberStyle::Arabic);
    assert_eq!(counters.section.ref_format, "{display_name} {number}");
  }

  #[test]
  fn default_counters_equation_uses_parens_ref_format() {
    let counters = Counters::default();
    assert_eq!(counters.equation.ref_format, "({number})");
  }

  #[test]
  fn default_counters_part_uses_roman_upper() {
    let counters = Counters::default();
    assert_eq!(counters.part.number_style, NumberStyle::RomanUpper);
  }

  #[test]
  fn deserializes_counter_entry() {
    // Arrange
    let toml = "
display_name = \"Figure\"
number_format = \"{chapter}.{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = []
";

    // Act
    let entry: CounterStyle = toml::from_str(toml).unwrap();

    // Assert
    assert_eq!(entry.display_name, "Figure");
    assert_eq!(entry.number_format, "{chapter}.{n}");
    assert_eq!(entry.number_style, NumberStyle::Arabic);
    assert_eq!(entry.ref_format, "{display_name} {number}");
  }

  #[test]
  fn rejects_renamed_format_key() {
    // Arrange: 旧キー `format` はハードリネームで廃止された（→ `number_format`）。
    // `deny_unknown_fields` が未知フィールドとして弾く（移行: 旧 config は即エラー）。
    let toml = "
display_name = \"Figure\"
format = \"{chapter}.{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = []
";

    // Act
    let result: Result<CounterStyle, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "旧キー `format` は未知フィールドとして拒否される");
  }

  #[test]
  fn counters_rejects_unknown_counter_name() {
    // Arrange: `example` は固定 9 種に含まれない
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
    // Arrange: `chapter` の resets に未知の名前 `example` を入れる
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
  fn counters_get_returns_matching_field() {
    let counters = Counters::default();
    assert!(std::ptr::eq(counters.get(CounterName::Chapter), &raw const counters.chapter));
    assert!(std::ptr::eq(counters.get(CounterName::Table), &raw const counters.table));
  }

  #[test]
  fn from_name_roundtrips_as_str_for_all() {
    // Arrange / Act / Assert — from_name と as_str が往復する
    for counter in CounterName::ALL {
      let name_str = counter.as_str();
      let recovered = CounterName::from_name(name_str);
      assert_eq!(recovered, Some(counter), "{name_str} から復元できるべき");
    }

    // Assert — 未知の名前は None を返す
    assert_eq!(CounterName::from_name("foo"), None);
  }
}
