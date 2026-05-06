//! 番号カウンタとラベル登録のレジストリ
//!
//! `\section`、`\begin{equation}`、`\begin{figure}` 等で発番される番号を一元管理し、
//! `\ref{label}` の解決に使うラベル → 番号の対応表を保持します。
//!
//! ## 2 パス評価との関係
//!
//! - **pass1**: `Evaluator::evaluate_children` がブロックを走査するたびに
//!   [`CounterRegistry::increment`] を呼び、`Heading.label` 等の任意ラベルが
//!   付いていれば [`CounterRegistry::register_label`] で登録する。
//! - **pass2**: `InlineNode::Ref { label, .. }` を [`CounterRegistry::resolve_label`] で
//!   解決し、`number: Some(裸の番号)` に書き換える。
//!
//! ## 数値書式について
//!
//! [`NumberFormat::Plain`] は単独カウンタの値（例: `"3"`）、
//! [`NumberFormat::Prefixed`] は親カウンタを `.` で連結した値（例: `"2.3.1"`）を返す。
//! 「3章」「第3部」のような装飾文字列は **lowering 側** で
//! `read_style::HeadingStyle.format` テンプレを介して付けるため、本レジストリは
//! 装飾を含まない裸の番号文字列のみを返すことに注意。
//!
//! ## TODO（実装本体タスクで解消）
//!
//! - `style.toml` に `[counters]` テーブルを追加し、[`CounterRegistry::from_style`] の
//!   中身を `default_for_seiran()` 委譲から実装に置き換える
//! - 図・数式・表のカウンタは現状 chapter 親で固定。`style.toml` でカスタマイズ可能にする

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::document::HeadingLevel;

/// 番号の表示形式
///
/// `Plain` は単独カウンタの値を 10 進数で返す（例: chapter は `"3"`）。
/// `Prefixed` は親カウンタを `.` 区切りで連結した値を返す（例: section は `"2.3"`）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum NumberFormat {
  /// 単独カウンタ。例: chapter は `"3"`
  Plain,
  /// 親カウンタチェーンと自身を `.` で結合。例: section は `"2.3"`
  Prefixed,
}

/// カウンタ 1 つの定義
///
/// 親子関係（`parent`）・リセット連鎖（`resets`）・別名（`alias_of`）を持つ。
/// `style.toml` に `[counters.<name>]` テーブルを足すことで将来カスタマイズ可能にする
/// 想定で、`HeadingStyle` と同じ `serde + garde` パターンに揃えている。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct CounterDef {
  /// カウンタ名（一意）
  pub name: String,
  /// 親カウンタ。`Prefixed` 形式の場合に `.` 連結のチェーンを構成する
  pub parent: Option<String>,
  /// 数値の表示形式
  pub format: NumberFormat,
  /// このカウンタが進んだときに 0 にリセットする下位カウンタ群
  pub resets: Vec<String>,
  /// 別名のソース。`Some(name)` の場合、`name` のカウンタと値を共有する
  pub alias_of: Option<String>,
}

/// pass1 で登録される、ラベル名から確定済み番号への解決結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLabel {
  /// 番号を発番したカウンタの正規名（alias 解決済み）
  pub counter: String,
  /// 確定済みの番号文字列（`format_number` の出力）
  pub number: String,
}

/// カウンタ群の状態と labels の登録状態を保持するレジストリ
#[derive(Debug, Default)]
#[allow(dead_code)]
pub(crate) struct CounterRegistry {
  /// 名前 → 定義
  counters: HashMap<String, CounterDef>,
  /// 名前 → 現在値（alias 解決後のキーで管理）
  values: HashMap<String, u32>,
  /// `\ref` 解決用テーブル。pass1 で登録、pass2 で参照する
  pub labels: HashMap<String, ResolvedLabel>,
}

#[allow(dead_code)]
impl CounterRegistry {
  /// seiran 既定のカウンタセットでレジストリを構築する
  ///
  /// 部・章・節・小節・段落・小段落、および図・数式・表（実装本体タスクで本格的に
  /// 利用される）のカウンタを既定値で登録する。
  #[must_use]
  pub fn default_for_seiran() -> Self {
    let defs = vec![
      CounterDef {
        name: "part".to_string(),
        parent: None,
        format: NumberFormat::Plain,
        resets: vec![
          "chapter".to_string(),
          "section".to_string(),
          "subsection".to_string(),
          "paragraph".to_string(),
          "subparagraph".to_string(),
        ],
        alias_of: None,
      },
      CounterDef {
        name: "chapter".to_string(),
        parent: None,
        format: NumberFormat::Plain,
        resets: vec![
          "section".to_string(),
          "subsection".to_string(),
          "paragraph".to_string(),
          "subparagraph".to_string(),
          "figure".to_string(),
          "equation".to_string(),
          "table".to_string(),
        ],
        alias_of: None,
      },
      CounterDef {
        name: "section".to_string(),
        parent: Some("chapter".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec![
          "subsection".to_string(),
          "paragraph".to_string(),
          "subparagraph".to_string(),
        ],
        alias_of: None,
      },
      CounterDef {
        name: "subsection".to_string(),
        parent: Some("section".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec!["paragraph".to_string(), "subparagraph".to_string()],
        alias_of: None,
      },
      CounterDef {
        name: "paragraph".to_string(),
        parent: Some("subsection".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec!["subparagraph".to_string()],
        alias_of: None,
      },
      CounterDef {
        name: "subparagraph".to_string(),
        parent: Some("paragraph".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec![],
        alias_of: None,
      },
      CounterDef {
        name: "figure".to_string(),
        parent: Some("chapter".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec![],
        alias_of: None,
      },
      CounterDef {
        name: "equation".to_string(),
        parent: Some("chapter".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec![],
        alias_of: None,
      },
      CounterDef {
        name: "table".to_string(),
        parent: Some("chapter".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec![],
        alias_of: None,
      },
    ];
    return Self::from_definitions(defs);
  }

  /// `read_style::Style` から `CounterRegistry` を構築する（シグネチャ予約）
  ///
  /// 現状 `Style` には `[counters]` テーブルが存在しないため、内部実装は
  /// `default_for_seiran()` への委譲のみ。`style.toml` 側のスキーマ拡張後に
  /// このメソッドだけを実装すれば配線完了する。
  #[must_use]
  pub fn from_style(_style: &read_style::Style) -> Self { return Self::default_for_seiran(); }

  /// 任意の定義列からレジストリを構築する（テスト・カスタム用）
  #[must_use]
  pub fn from_definitions(defs: Vec<CounterDef>) -> Self {
    let mut counters = HashMap::new();
    let mut values = HashMap::new();
    for def in defs {
      values.entry(def.name.clone()).or_insert(0);
      counters.insert(def.name.clone(), def);
    }
    return Self {
      counters,
      values,
      labels: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、書式化済みの番号文字列を返す
  ///
  /// alias 解決を経由するため、別名カウンタを進めると元のカウンタも進む。
  pub fn increment(&mut self, name: &str) -> String {
    let canonical = self.resolve_alias(name);
    let resets = self.counters.get(&canonical).map(|d| d.resets.clone()).unwrap_or_default();

    *self.values.entry(canonical.clone()).or_insert(0) += 1;
    for r in resets {
      let r_canonical = self.resolve_alias(&r);
      if let Some(v) = self.values.get_mut(&r_canonical) {
        *v = 0;
      }
    }

    return self.format_number(&canonical);
  }

  /// 現在のカウンタ値を `format` に従って書式化する
  #[must_use]
  pub fn format_number(&self, name: &str) -> String {
    let canonical = self.resolve_alias(name);
    let Some(def) = self.counters.get(&canonical) else {
      return String::new();
    };
    match def.format {
      NumberFormat::Plain => return self.values.get(&canonical).copied().unwrap_or(0).to_string(),
      NumberFormat::Prefixed => return self.format_prefixed(&canonical),
    }
  }

  /// 親チェーンを root → leaf の順に書式化する
  fn format_prefixed(&self, name: &str) -> String {
    let mut chain: Vec<u32> = Vec::new();
    let mut cursor = Some(name.to_string());
    while let Some(n) = cursor {
      chain.push(self.values.get(&n).copied().unwrap_or(0));
      cursor = self.counters.get(&n).and_then(|d| d.parent.clone()).map(|p| self.resolve_alias(&p));
    }
    chain.reverse();
    return chain.iter().map(u32::to_string).collect::<Vec<_>>().join(".");
  }

  /// alias を辿って正規のカウンタ名を返す
  fn resolve_alias(&self, name: &str) -> String {
    let mut current = name.to_string();
    let mut hops = 0_u32;
    // 循環 alias を踏んだ場合の保険として、定義数を上限にする
    let limit = self.counters.len() as u32 + 1;
    while hops < limit {
      let Some(def) = self.counters.get(&current) else {
        break;
      };
      let Some(target) = &def.alias_of else {
        break;
      };
      current = target.clone();
      hops += 1;
    }
    return current;
  }

  /// pass1 で `\section[label=sec:intro]{...}` などからラベルを登録する
  pub fn register_label(&mut self, label: impl Into<String>, counter: impl Into<String>, number: impl Into<String>) {
    let counter = self.resolve_alias(&counter.into());
    self.labels.insert(
      label.into(),
      ResolvedLabel {
        counter,
        number: number.into(),
      },
    );
  }

  /// pass2 で `\ref{label}` を解決して番号文字列を返す
  ///
  /// 未登録ラベルの場合は `None`。呼び出し側でエラー化する想定。
  #[must_use]
  pub fn resolve_label(&self, label: &str) -> Option<&str> { return self.labels.get(label).map(|r| r.number.as_str()); }

  /// 見出しレベルから seiran 既定カウンタ名を返す
  #[must_use]
  pub fn counter_name_for_heading(level: HeadingLevel) -> &'static str {
    return match level {
      HeadingLevel::Part => "part",
      HeadingLevel::Chapter => "chapter",
      HeadingLevel::Section => "section",
      HeadingLevel::Subsection => "subsection",
      HeadingLevel::Paragraph => "paragraph",
      HeadingLevel::Subparagraph => "subparagraph",
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn counter_registry_increment_format() {
    let mut r = CounterRegistry::default_for_seiran();
    assert_eq!(r.increment("chapter"), "1");
    assert_eq!(r.increment("section"), "1.1");
    assert_eq!(r.increment("section"), "1.2");
    assert_eq!(r.format_number("chapter"), "1");
    assert_eq!(r.format_number("section"), "1.2");
  }

  #[test]
  fn counter_registry_alias_shared_numbering() {
    // subsubsection を section の alias として定義 → section と番号を共有
    let defs = vec![
      CounterDef {
        name: "chapter".to_string(),
        parent: None,
        format: NumberFormat::Plain,
        resets: vec!["section".to_string()],
        alias_of: None,
      },
      CounterDef {
        name: "section".to_string(),
        parent: Some("chapter".to_string()),
        format: NumberFormat::Prefixed,
        resets: vec![],
        alias_of: None,
      },
      CounterDef {
        name: "subsubsection".to_string(),
        parent: None,
        format: NumberFormat::Prefixed,
        resets: vec![],
        alias_of: Some("section".to_string()),
      },
    ];
    let mut r = CounterRegistry::from_definitions(defs);
    r.increment("chapter");
    let a = r.increment("section"); // section -> 1.1
    let b = r.increment("subsubsection"); // alias で section が進む -> 1.2
    assert_eq!(a, "1.1");
    assert_eq!(b, "1.2");
  }

  #[test]
  fn counter_registry_section_reset() {
    let mut r = CounterRegistry::default_for_seiran();
    r.increment("chapter"); // chapter = 1
    r.increment("section"); // section = 1.1
    r.increment("section"); // section = 1.2
    r.increment("chapter"); // chapter = 2、section は 0 にリセット
    let next = r.increment("section");
    assert_eq!(next, "2.1");
  }

  #[test]
  fn evaluate_ref_pass2_resolves_number() {
    // CounterRegistry レベルでラベル登録 → 解決
    let mut r = CounterRegistry::default_for_seiran();
    r.increment("chapter");
    let chapter_number = r.format_number("chapter");
    r.register_label("ch:intro", "chapter", chapter_number.clone());

    assert_eq!(r.resolve_label("ch:intro"), Some(chapter_number.as_str()));
  }

  #[test]
  fn evaluate_unknown_label_errors() {
    let r = CounterRegistry::default_for_seiran();
    assert!(r.resolve_label("nonexistent").is_none());
  }

  #[test]
  fn from_style_delegates_to_default() {
    // シグネチャ予約: 現状は default_for_seiran() と同じ動作のみ確認
    let style = read_style::Style::default();
    let mut r = CounterRegistry::from_style(&style);
    assert_eq!(r.increment("chapter"), "1");
  }
}
