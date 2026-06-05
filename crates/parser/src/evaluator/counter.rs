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
//! ## カウンタ定義のソース
//!
//! カウンタ定義の真のソースは `read_style::Style.counters` テーブル。
//! [`CounterRegistry::from_style`] が [`read_style::CounterEntry`] を内部表現の
//! [`CounterDef`] にマップする。既定 9 種（part〜table）も `Style::default()` が
//! `read_style::default_counters()` 経由で供給するため、parser 側に同じ定義を
//! 重複して持たない。

use std::collections::HashMap;

use read_style::{CounterEntry, NumberFormat, Style};

use crate::document::HeadingLevel;

/// カウンタ 1 つの内部表現
///
/// [`read_style::CounterEntry`]（カウンタ定義と別名の untagged enum）を平坦化したもの。
/// `alias_of` を `CounterDef` 自身のフィールドに畳むことで、`HashMap<String, CounterDef>`
/// に対する統一的なルックアップが可能になる。
#[derive(Debug, Clone)]
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
  /// 既定値は `read_style::Style::default()` が `default_counters()` 経由で供給する
  /// 9 種（part / chapter / section / subsection / paragraph / subparagraph /
  /// figure / equation / table）。
  #[must_use]
  pub fn default_for_seiran() -> Self { return Self::from_style(&Style::default()); }

  /// `read_style::Style` からレジストリを構築する
  ///
  /// `Style.counters` の各エントリを [`CounterDef`] に変換して [`Self::from_definitions`] に
  /// 渡す。`CounterEntry::Counter` は通常定義に、`CounterEntry::Alias` は `alias_of` のみを
  /// 持つ `CounterDef` にマップする。`display_name`（"Figure" / "図" 等の装飾文字列）は
  /// lowering 側の責務のため捨てる。
  #[must_use]
  pub fn from_style(style: &Style) -> Self {
    let defs: Vec<CounterDef> = style
      .core
      .counters
      .iter()
      .map(|(name, entry)| match entry {
        CounterEntry::Counter(def) => CounterDef {
          name: name.clone(),
          parent: def.parent.clone(),
          format: def.format,
          resets: def.resets.clone(),
          alias_of: None,
        },
        CounterEntry::Alias(alias) => CounterDef {
          name: name.clone(),
          parent: None,
          format: NumberFormat::Plain,
          resets: Vec::new(),
          alias_of: Some(alias.alias_of.clone()),
        },
      })
      .collect();
    return Self::from_definitions(defs);
  }

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
  use read_style::{AliasDef, CounterEntry, CounterStyle, NumberFormat, Style};

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
  fn from_style_includes_custom_counter_definition() {
    // Arrange: 既定 Style に独自カウンタ "example" を追加（chapter 親、prefixed）
    let mut style = Style::default();
    style.core.counters.insert(
      "example".to_string(),
      CounterEntry::Counter(CounterStyle {
        display_name: "Example".to_string(),
        parent: Some("chapter".to_string()),
        format: NumberFormat::Prefixed,
        resets: Vec::new(),
      }),
    );

    // Act
    let mut r = CounterRegistry::from_style(&style);
    r.increment("chapter");
    let n1 = r.increment("example");
    let n2 = r.increment("example");

    // Assert: chapter 親が反映されており、prefixed で "1.1" → "1.2" と進む
    assert_eq!(n1, "1.1");
    assert_eq!(n2, "1.2");
  }

  #[test]
  fn from_style_maps_alias_entry_to_canonical_counter() {
    // Arrange: 既定 Style に "fig" を "figure" の別名として追加
    let mut style = Style::default();
    style.core.counters.insert(
      "fig".to_string(),
      CounterEntry::Alias(AliasDef {
        alias_of: "figure".to_string(),
      }),
    );

    // Act: alias 経由で進めても figure 本体が進むことを確認
    let mut r = CounterRegistry::from_style(&style);
    r.increment("chapter");
    let via_alias = r.increment("fig"); // alias 経由
    let via_canonical = r.increment("figure"); // 本体経由

    // Assert: 同じ figure カウンタを共有しているので連番になる
    assert_eq!(via_alias, "1.1");
    assert_eq!(via_canonical, "1.2");
  }

  #[test]
  fn from_style_with_default_style_matches_default_for_seiran() {
    // Arrange / Act
    let mut from_default = CounterRegistry::from_style(&Style::default());
    let mut from_helper = CounterRegistry::default_for_seiran();

    // Assert: 既定 Style 経由と default_for_seiran() が同じ振る舞いをする
    assert_eq!(from_default.increment("chapter"), from_helper.increment("chapter"));
    assert_eq!(from_default.increment("section"), from_helper.increment("section"));
  }
}
