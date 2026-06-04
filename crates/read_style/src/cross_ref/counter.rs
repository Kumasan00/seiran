//! カウンタ（chapter / section / figure 等）のスタイル設定型。

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Serialize};

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
  /// 別名のソース。`Some(name)` の場合、`name` のカウンタと値を共有する
  pub alias_of: Option<String>,
}

impl CounterStyle {
  /// 新しい [`CounterStyle`] を作成するヘルパー
  #[must_use]
  pub(crate) fn new(
    display_name: &str,
    parent: Option<&str>,
    format: NumberFormat,
    resets: &[&str],
    alias_of: Option<&str>,
  ) -> Self {
    return Self {
      display_name: display_name.to_string(),
      parent: parent.map(str::to_string),
      format,
      resets: resets.iter().map(|s| (*s).to_string()).collect(),
      alias_of: alias_of.map(str::to_string),
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
pub fn default_counters() -> HashMap<String, CounterStyle> {
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
        None,
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
        None,
      ),
    ),
    (
      "section",
      CounterStyle::new(
        "Section",
        Some("chapter"),
        NumberFormat::Prefixed,
        &["subsection", "paragraph", "subparagraph"],
        None,
      ),
    ),
    (
      "subsection",
      CounterStyle::new("Subsection", Some("section"), NumberFormat::Prefixed, &["paragraph", "subparagraph"], None),
    ),
    (
      "paragraph",
      CounterStyle::new("Paragraph", Some("subsection"), NumberFormat::Prefixed, &["subparagraph"], None),
    ),
    (
      "subparagraph",
      CounterStyle::new("Subparagraph", Some("paragraph"), NumberFormat::Prefixed, &[], None),
    ),
    ("figure", CounterStyle::new("Figure", Some("chapter"), NumberFormat::Prefixed, &[], None)),
    ("equation", CounterStyle::new("Equation", Some("chapter"), NumberFormat::Prefixed, &[], None)),
    ("table", CounterStyle::new("Table", Some("chapter"), NumberFormat::Prefixed, &[], None)),
  ];
  return entries.into_iter().map(|(name, style)| (name.to_string(), style)).collect();
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{CounterStyle, NumberFormat, default_counters};

  #[test]
  fn validate_accepts_minimal_counter() {
    let counter = CounterStyle::new("Figure", Some("chapter"), NumberFormat::Prefixed, &[], None);
    assert!(counter.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_display_name() {
    let counter = CounterStyle::new("", None, NumberFormat::Plain, &[], None);
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
    assert_eq!(section.parent.as_deref(), Some("chapter"));
    assert_eq!(section.format, NumberFormat::Prefixed);
  }
}
