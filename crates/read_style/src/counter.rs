//! カウンタ（chapter / section / figure 等）のユーザ向けスタイル設定型。
//!
//! `parser::evaluator::counter::CounterRegistry` がランタイムでカウンタを管理しますが、
//! 本モジュールは TOML スキーマとしてカウンタの宣言を受け取るための型を定義します。
//! `CounterRegistry::from_style` がここで宣言された [`CounterStyle`] を実行時表現に変換します。

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 番号の表示形式
///
/// `Plain` は単独カウンタの値を返す（例: chapter は `"3"`）。
/// `Prefixed` は親カウンタを `.` 区切りで連結した値を返す（例: section は `"2.3"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum NumberFormat {
  /// 単独カウンタ。例: chapter は `"3"`
  Plain,
  /// 親カウンタチェーンと自身を `.` で結合。例: section は `"2.3"`
  Prefixed,
}

/// 1 つのカウンタ定義（TOML スキーマ）
///
/// `display_name` で「図」「Figure」のような i18n 文字列を指定し、
/// `parent` / `resets` / `alias_of` で番号体系の構造を表現します。
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

/// seiran 既定のカウンタセットを返す
///
/// `parser::evaluator::counter::CounterRegistry::default_for_seiran` と同じセット
/// （part / chapter / section / subsection / paragraph / subparagraph / figure / equation / table）を
/// 英語の `display_name` で初期化します。
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
    // Arrange
    let counter = CounterStyle::new("Figure", Some("chapter"), NumberFormat::Prefixed, &[], None);

    // Act / Assert
    assert!(counter.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_display_name() {
    // Arrange
    let counter = CounterStyle::new("", None, NumberFormat::Plain, &[], None);

    // Act / Assert
    assert!(counter.validate().is_err());
  }

  #[test]
  fn default_counters_contains_expected_names() {
    // Arrange / Act
    let counters = default_counters();

    // Assert
    for expected in ["part", "chapter", "section", "figure", "equation", "table"] {
      assert!(counters.contains_key(expected), "missing counter: {expected}");
    }
  }

  #[test]
  fn default_counters_section_has_chapter_parent() {
    // Arrange / Act
    let counters = default_counters();

    // Assert
    let section = counters.get("section").expect("section counter");
    assert_eq!(section.parent.as_deref(), Some("chapter"));
    assert_eq!(section.format, NumberFormat::Prefixed);
  }

  #[test]
  fn default_counters_chapter_resets_figure_equation_table() {
    // Arrange / Act
    let counters = default_counters();

    // Assert
    let chapter = counters.get("chapter").expect("chapter counter");
    for expected in ["figure", "equation", "table"] {
      assert!(chapter.resets.iter().any(|r| r == expected), "chapter should reset {expected}");
    }
  }
}
