//! カウンタ（chapter / section / figure 等）のスタイル設定型。
//!
//! TOML 上では `[counters.<name>]` のキーに 2 種類の値が来る:
//!
//! 1. カウンタ定義（[`CounterStyle`]）: `display_name` / `parent` / `format` / `resets` を持つ
//! 2. 別名（`{ alias_of = "<canonical_name>" }`）: 既存カウンタへのリンクのみ
//!
//! 排他関係は [`CounterEntry`] の `#[serde(untagged)]` で表現する。

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Serialize};

/// `[counters.<name>]` テーブルが取れる 2 形態。
///
/// `Alias` は `alias_of` のみ、`Counter` は `CounterStyle` のフィールドのみを受け入れる。
/// それぞれの内側の struct に `#[serde(deny_unknown_fields)]` を付けることで混在を弾く。
/// `untagged` 列挙体で struct variant に `deny_unknown_fields` を効かせる serde の制約
/// 回避のため、`Alias` は独立 struct [`AliasDef`] を経由する。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(untagged)]
pub enum CounterEntry {
  /// 別名エントリ。`alias_of` で示すカウンタと同じ値を共有する。
  Alias(#[garde(dive)] AliasDef),
  /// カウンタ定義エントリ。
  Counter(#[garde(dive)] CounterStyle),
}

/// `{ alias_of = "<name>" }` のエントリ表現。
///
/// `#[serde(deny_unknown_fields)]` でカウンタ定義フィールド（`display_name` など）の混入を弾く。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct AliasDef {
  /// 別名のソース（参照先のカウンタ名）
  #[garde(length(chars, min = 1))]
  pub alias_of: String,
}

/// 1 つのカウンタ定義（TOML スキーマ）
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct CounterStyle {
  /// 表示名（例: `"Figure"`、`"図"`）。`\ref` 解決時の種別名としても使われる
  #[garde(length(chars, min = 1))]
  pub display_name: String,
  /// 親カウンタ。`Prefixed` 形式の場合に `.` 連結のチェーンを構成する
  pub parent: Option<String>,
  /// 数値の表示形式
  pub format: NumberFormat,
  /// このカウンタが進んだときに 0 にリセットする下位カウンタ群
  pub resets: Vec<String>,
}

impl CounterStyle {
  /// 新しい [`CounterStyle`] を作成するヘルパー
  #[must_use]
  pub(crate) fn new(display_name: &str, parent: Option<&str>, format: NumberFormat, resets: &[&str]) -> Self {
    return Self {
      display_name: display_name.to_string(),
      parent: parent.map(str::to_string),
      format,
      resets: resets.iter().map(|s| (*s).to_string()).collect(),
    };
  }
}

/// 番号の表示形式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum NumberFormat {
  /// 単独カウンタ。例: chapter は `"3"`
  Plain,
  /// 親カウンタチェーンと自身を `.` で結合。例: section は `"2.3"`
  Prefixed,
}

/// seiran 既定のカウンタセットを返す
#[must_use]
pub fn default_counters() -> HashMap<String, CounterEntry> {
  let entries: [(&str, CounterStyle); 9] = [
    (
      "part",
      CounterStyle::new(
        "Part",
        None,
        NumberFormat::Plain,
        &[
          "chapter",
          "section",
          "subsection",
          "paragraph",
          "subparagraph",
        ],
      ),
    ),
    (
      "chapter",
      CounterStyle::new(
        "Chapter",
        None,
        NumberFormat::Plain,
        &[
          "section",
          "subsection",
          "paragraph",
          "subparagraph",
          "figure",
          "equation",
          "table",
        ],
      ),
    ),
    (
      "section",
      CounterStyle::new(
        "Section",
        Some("chapter"),
        NumberFormat::Prefixed,
        &["subsection", "paragraph", "subparagraph"],
      ),
    ),
    (
      "subsection",
      CounterStyle::new("Subsection", Some("section"), NumberFormat::Prefixed, &["paragraph", "subparagraph"]),
    ),
    (
      "paragraph",
      CounterStyle::new("Paragraph", Some("subsection"), NumberFormat::Prefixed, &["subparagraph"]),
    ),
    ("subparagraph", CounterStyle::new("Subparagraph", Some("paragraph"), NumberFormat::Prefixed, &[])),
    ("figure", CounterStyle::new("Figure", Some("chapter"), NumberFormat::Prefixed, &[])),
    ("equation", CounterStyle::new("Equation", Some("chapter"), NumberFormat::Prefixed, &[])),
    ("table", CounterStyle::new("Table", Some("chapter"), NumberFormat::Prefixed, &[])),
  ];
  return entries.into_iter().map(|(name, style)| (name.to_string(), CounterEntry::Counter(style))).collect();
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{CounterEntry, CounterStyle, NumberFormat, default_counters};
  use crate::common::counter::AliasDef;

  #[test]
  fn validate_accepts_minimal_counter() {
    let counter = CounterStyle::new("Figure", Some("chapter"), NumberFormat::Prefixed, &[]);
    assert!(counter.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_display_name() {
    let counter = CounterStyle::new("", None, NumberFormat::Plain, &[]);
    assert!(counter.validate().is_err());
  }

  #[test]
  fn default_counters_contains_expected_names() {
    let counters = default_counters();
    for expected in ["part", "chapter", "section", "figure", "equation", "table"] {
      assert!(counters.contains_key(expected), "missing counter: {expected}");
    }
  }

  #[test]
  fn default_counters_section_has_chapter_parent() {
    let counters = default_counters();
    let section = counters.get("section").expect("section counter");
    match section {
      CounterEntry::Counter(def) => {
        assert_eq!(def.parent.as_deref(), Some("chapter"));
        assert_eq!(def.format, NumberFormat::Prefixed);
      },
      CounterEntry::Alias { .. } => panic!("section は alias であってはならない"),
    }
  }

  #[test]
  fn deserializes_alias_entry() {
    // Arrange
    let toml = "alias_of = \"figure\"\n";

    // Act
    let entry: CounterEntry = toml::from_str(toml).unwrap();

    // Assert
    match entry {
      CounterEntry::Alias(AliasDef { alias_of }) => assert_eq!(alias_of, "figure"),
      CounterEntry::Counter(_) => panic!("alias_of のみのエントリは Alias variant にマッチすべき"),
    }
  }

  #[test]
  fn deserializes_counter_entry() {
    // Arrange
    let toml = "
display_name = \"Figure\"
parent = \"chapter\"
format = \"prefixed\"
resets = []
";

    // Act
    let entry: CounterEntry = toml::from_str(toml).unwrap();

    // Assert
    match entry {
      CounterEntry::Counter(def) => {
        assert_eq!(def.display_name, "Figure");
        assert_eq!(def.parent.as_deref(), Some("chapter"));
        assert_eq!(def.format, NumberFormat::Prefixed);
      },
      CounterEntry::Alias { .. } => panic!("カウンタ定義は Counter variant にマッチすべき"),
    }
  }

  #[test]
  fn rejects_mixing_alias_with_counter_fields() {
    // Arrange: alias_of と他のカウンタ定義フィールドを同時に書くと、どちらの variant にも
    // マッチしない（Alias は alias_of のみ、Counter は alias_of を持たない）
    let toml = "
alias_of = \"figure\"
display_name = \"Fig\"
";

    // Act
    let result: Result<CounterEntry, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "alias_of とカウンタ定義フィールドの混在は拒否されるべき: {result:?}");
  }
}
