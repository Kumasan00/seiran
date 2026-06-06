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
//! ## 番号書式について
//!
//! 各カウンタは [`read_style::CounterStyle`] の `format` テンプレート（例: `"{n}"`、
//! `"{chapter}.{n}"`）に従って文字列化される。`{n}` は自身のカウンタ値、`{<name>}` は
//! 他カウンタの値を参照先カウンタの [`read_style::NumberStyle`] でレンダリングする
//! （再帰展開はしない）。「3章」「第3部」のような装飾文字列は **lowering 側** で
//! `read_style::HeadingStyle.format` テンプレを介して付ける。
//!
//! ## `\ref` の書式について
//!
//! `\ref{label}` の表示は [`read_style::CounterStyle::ref_format`] テンプレートで決まる
//! （例: `"{display_name} {number}"` → `"Section 1.2"`、`"({number})"` → `"(1.2)"`）。
//! `register_label` 時点でテンプレートを適用するため、`resolve_label` は整形済み文字列を
//! 返す（呼び出し側は装飾を気にせず使える）。
//!
//! ## カウンタ定義のソース
//!
//! カウンタ定義の真のソースは `read_style::Style.counters` テーブル。
//! [`CounterRegistry::from_style`] が [`read_style::Counters`] の各フィールドを内部の
//! [`CounterStates`] に複製する。既定 9 種（part〜table）も `Style::default()` が
//! `read_style::Counters::default()` 経由で供給するため、parser 側に同じ定義を
//! 重複して持たない。

use std::collections::HashMap;

use read_style::{CounterName, CounterStyle, Counters, Style};

use crate::document::HeadingLevel;

/// pass1 で登録される、ラベル名から確定済み番号への解決結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLabel {
  /// 番号を発番したカウンタ
  pub counter: CounterName,
  /// 確定済みの番号文字列（`format_number` の出力に `ref_format` を適用したもの）
  pub number: String,
}

/// カウンタ 1 つの実行時状態（定義 + 現在値）
#[derive(Debug, Clone)]
pub(crate) struct CounterState {
  /// `read_style` から取り込んだスタイル定義
  pub def: CounterStyle,
  /// 現在のカウンタ値（初期値 0、`increment` で 1 ずつ進む）
  pub value: u32,
}

impl CounterState {
  fn new(def: CounterStyle) -> Self { return Self { def, value: 0 }; }
}

/// 固定 9 種のカウンタ状態を保持する struct（[`Counters`] と同じ形）
///
/// `read_style::Counters` がフィールド型から `HashMap` ベースの定義テーブルへ変わったのに合わせ、
/// parser 側でも `HashMap<String, _>` ではなく名前付きフィールドで保持する。
#[derive(Debug, Clone)]
pub(crate) struct CounterStates {
  pub part: CounterState,
  pub chapter: CounterState,
  pub section: CounterState,
  pub subsection: CounterState,
  pub paragraph: CounterState,
  pub subparagraph: CounterState,
  pub figure: CounterState,
  pub equation: CounterState,
  pub table: CounterState,
}

impl CounterStates {
  /// `read_style::Counters` の各フィールドを `CounterState` にラップして取り込む
  #[must_use]
  pub fn from_counters(counters: &Counters) -> Self {
    return Self {
      part: CounterState::new(counters.part.clone()),
      chapter: CounterState::new(counters.chapter.clone()),
      section: CounterState::new(counters.section.clone()),
      subsection: CounterState::new(counters.subsection.clone()),
      paragraph: CounterState::new(counters.paragraph.clone()),
      subparagraph: CounterState::new(counters.subparagraph.clone()),
      figure: CounterState::new(counters.figure.clone()),
      equation: CounterState::new(counters.equation.clone()),
      table: CounterState::new(counters.table.clone()),
    };
  }

  /// 指定カウンタの状態への不変参照を返す（9 種固定のため必ず存在する）
  #[must_use]
  pub fn get(&self, name: CounterName) -> &CounterState {
    return match name {
      CounterName::Part => &self.part,
      CounterName::Chapter => &self.chapter,
      CounterName::Section => &self.section,
      CounterName::Subsection => &self.subsection,
      CounterName::Paragraph => &self.paragraph,
      CounterName::Subparagraph => &self.subparagraph,
      CounterName::Figure => &self.figure,
      CounterName::Equation => &self.equation,
      CounterName::Table => &self.table,
    };
  }

  /// 指定カウンタの状態への可変参照を返す
  pub fn get_mut(&mut self, name: CounterName) -> &mut CounterState {
    return match name {
      CounterName::Part => &mut self.part,
      CounterName::Chapter => &mut self.chapter,
      CounterName::Section => &mut self.section,
      CounterName::Subsection => &mut self.subsection,
      CounterName::Paragraph => &mut self.paragraph,
      CounterName::Subparagraph => &mut self.subparagraph,
      CounterName::Figure => &mut self.figure,
      CounterName::Equation => &mut self.equation,
      CounterName::Table => &mut self.table,
    };
  }
}

/// カウンタ群の状態と labels の登録状態を保持するレジストリ
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CounterRegistry {
  /// 9 種のカウンタ状態
  counters: CounterStates,
  /// `\ref` 解決用テーブル。pass1 で登録、pass2 で参照する
  pub labels: HashMap<String, ResolvedLabel>,
}

#[allow(dead_code)]
impl CounterRegistry {
  /// seiran 既定のカウンタセットでレジストリを構築する
  ///
  /// 既定値は `read_style::Style::default()` が `Counters::default()` 経由で供給する
  /// 9 種（part / chapter / section / subsection / paragraph / subparagraph /
  /// figure / equation / table）。
  #[must_use]
  pub fn default_for_seiran() -> Self { return Self::from_style(&Style::default()); }

  /// `read_style::Style` からレジストリを構築する
  #[must_use]
  pub fn from_style(style: &Style) -> Self { return Self::from_counters(&style.core.counters); }

  /// `read_style::Counters` から直接レジストリを構築する（テスト・カスタム用）
  #[must_use]
  pub fn from_counters(counters: &Counters) -> Self {
    return Self {
      counters: CounterStates::from_counters(counters),
      labels: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、書式化済みの番号文字列を返す
  pub fn increment(&mut self, name: CounterName) -> String {
    let resets = self.counters.get(name).def.resets.clone();

    self.counters.get_mut(name).value += 1;
    for r in resets {
      self.counters.get_mut(r).value = 0;
    }

    return self.format_number(name);
  }

  /// 現在のカウンタ値を `format` テンプレートに従って書式化する
  #[must_use]
  pub fn format_number(&self, name: CounterName) -> String {
    let def = &self.counters.get(name).def;
    return self.expand_template(&def.format, name);
  }

  /// テンプレートのプレースホルダ `{n}` / `{<counter_name>}` を値で置換する
  ///
  /// - `{n}` は `self_name` カウンタの値を、その `number_style` でレンダリングする
  /// - `{<name>}` は参照先カウンタの値を、参照先の `number_style` でレンダリングする
  ///   （テンプレートは再帰展開しない）
  /// - 未知のカウンタ名（9 種以外）は空文字列に置換する
  fn expand_template(&self, template: &str, self_name: CounterName) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
      if c != '{' {
        out.push(c);
        continue;
      }
      let mut name = String::new();
      let mut closed = false;
      while let Some(&nc) = chars.peek() {
        chars.next();
        if nc == '}' {
          closed = true;
          break;
        }
        name.push(nc);
      }
      if !closed {
        // 閉じ括弧なしの `{...` はリテラル扱いとして残す
        out.push('{');
        out.push_str(&name);
        continue;
      }
      let target = if name == "n" {
        Some(self_name)
      } else {
        parse_counter_name(&name)
      };
      if let Some(target) = target {
        out.push_str(&self.render_counter_value(target));
      }
    }
    return out;
  }

  /// カウンタの「現在値を自身の `number_style` で描画した文字列」を返す
  fn render_counter_value(&self, name: CounterName) -> String {
    let state = self.counters.get(name);
    return state.def.number_style.render(state.value);
  }

  /// pass1 で `\section[label=sec:intro]{...}` などからラベルを登録する
  ///
  /// 渡された `number`（裸の番号）にカウンタの `ref_format` を適用し、`\ref` 時の表示
  /// 文字列を作って保存する（例: `"1.2"` → `"Section 1.2"`、`"({number})"` 形式なら `"(1.2)"`）。
  pub fn register_label(&mut self, label: impl Into<String>, counter: CounterName, number: impl Into<String>) {
    let def = &self.counters.get(counter).def;
    let formatted = expand_ref_format(&def.ref_format, &number.into(), &def.display_name);
    self.labels.insert(
      label.into(),
      ResolvedLabel {
        counter,
        number: formatted,
      },
    );
  }

  /// pass2 で `\ref{label}` を解決して番号文字列を返す
  ///
  /// 未登録ラベルの場合は `None`。呼び出し側でエラー化する想定。
  #[must_use]
  pub fn resolve_label(&self, label: &str) -> Option<&str> { return self.labels.get(label).map(|r| r.number.as_str()); }

  /// 見出しレベルから seiran 既定の [`CounterName`] を返す
  #[must_use]
  pub fn counter_name_for_heading(level: HeadingLevel) -> CounterName {
    return match level {
      HeadingLevel::Part => CounterName::Part,
      HeadingLevel::Chapter => CounterName::Chapter,
      HeadingLevel::Section => CounterName::Section,
      HeadingLevel::Subsection => CounterName::Subsection,
      HeadingLevel::Paragraph => CounterName::Paragraph,
      HeadingLevel::Subparagraph => CounterName::Subparagraph,
    };
  }
}

/// `snake_case` のカウンタ名文字列を [`CounterName`] に解決する
///
/// テンプレート内の `{chapter}` のような自由記述プレースホルダから enum に戻すために使う。
/// 9 種以外の文字列は `None` を返す。
fn parse_counter_name(s: &str) -> Option<CounterName> {
  return match s {
    "part" => Some(CounterName::Part),
    "chapter" => Some(CounterName::Chapter),
    "section" => Some(CounterName::Section),
    "subsection" => Some(CounterName::Subsection),
    "paragraph" => Some(CounterName::Paragraph),
    "subparagraph" => Some(CounterName::Subparagraph),
    "figure" => Some(CounterName::Figure),
    "equation" => Some(CounterName::Equation),
    "table" => Some(CounterName::Table),
    _ => None,
  };
}

/// `ref_format` テンプレートを適用して `\ref` の表示文字列を作る
///
/// 認識するプレースホルダは `{number}`（裸の番号）と `{display_name}`（種別名）のみ。
/// 未知のプレースホルダや閉じ括弧の欠落はリテラル扱いで残す。
fn expand_ref_format(template: &str, number: &str, display_name: &str) -> String {
  let mut out = String::new();
  let mut chars = template.chars().peekable();
  while let Some(c) = chars.next() {
    if c != '{' {
      out.push(c);
      continue;
    }
    let mut name = String::new();
    let mut closed = false;
    while let Some(&nc) = chars.peek() {
      chars.next();
      if nc == '}' {
        closed = true;
        break;
      }
      name.push(nc);
    }
    if !closed {
      out.push('{');
      out.push_str(&name);
      continue;
    }
    match name.as_str() {
      "number" => out.push_str(number),
      "display_name" => out.push_str(display_name),
      _ => {
        // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
        out.push('{');
        out.push_str(&name);
        out.push('}');
      },
    }
  }
  return out;
}

#[cfg(test)]
mod tests {
  use read_style::{CounterName, CounterStyle, Counters, NumberStyle, Style};

  use super::*;

  #[test]
  fn counter_registry_increment_format() {
    let mut r = CounterRegistry::default_for_seiran();
    assert_eq!(r.increment(CounterName::Chapter), "1");
    assert_eq!(r.increment(CounterName::Section), "1.1");
    assert_eq!(r.increment(CounterName::Section), "1.2");
    assert_eq!(r.format_number(CounterName::Chapter), "1");
    assert_eq!(r.format_number(CounterName::Section), "1.2");
  }

  #[test]
  fn counter_registry_part_uses_roman_upper() {
    let mut r = CounterRegistry::default_for_seiran();
    assert_eq!(r.increment(CounterName::Part), "I");
    assert_eq!(r.increment(CounterName::Part), "II");
  }

  #[test]
  fn counter_registry_section_reset() {
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter); // chapter = 1
    r.increment(CounterName::Section); // section = 1.1
    r.increment(CounterName::Section); // section = 1.2
    r.increment(CounterName::Chapter); // chapter = 2、section は 0 にリセット
    let next = r.increment(CounterName::Section);
    assert_eq!(next, "2.1");
  }

  #[test]
  fn template_with_literal_decoration() {
    // Arrange: chapter を "第{n}章" 形式で発番する
    let counters = Counters {
      chapter: CounterStyle {
        display_name: "Chapter".to_string(),
        format: "第{n}章".to_string(),
        number_style: NumberStyle::Arabic,
        ref_format: "{number}".to_string(),
        resets: vec![],
      },
      ..Counters::default()
    };
    let mut r = CounterRegistry::from_counters(&counters);

    // Act / Assert
    assert_eq!(r.increment(CounterName::Chapter), "第1章");
    assert_eq!(r.increment(CounterName::Chapter), "第2章");
  }

  #[test]
  fn template_cross_counter_uses_target_number_style() {
    // Arrange: part を roman、chapter は part を参照する arabic
    let counters = Counters {
      part: CounterStyle {
        display_name: "Part".to_string(),
        format: "{n}".to_string(),
        number_style: NumberStyle::RomanUpper,
        ref_format: "{number}".to_string(),
        resets: vec![CounterName::Chapter],
      },
      chapter: CounterStyle {
        display_name: "Chapter".to_string(),
        format: "{part}-{n}".to_string(),
        number_style: NumberStyle::Arabic,
        ref_format: "{number}".to_string(),
        resets: vec![],
      },
      ..Counters::default()
    };
    let mut r = CounterRegistry::from_counters(&counters);

    // Act
    r.increment(CounterName::Part); // I
    r.increment(CounterName::Part); // II
    let ch = r.increment(CounterName::Chapter);

    // Assert: part は Roman、chapter 自身は Arabic で展開される
    assert_eq!(ch, "II-1");
  }

  #[test]
  fn evaluate_ref_pass2_applies_ref_format() {
    // 既定 chapter は ref_format = "{display_name} {number}" なので "Chapter 1" が返る
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter);
    let bare = r.format_number(CounterName::Chapter);
    r.register_label("ch:intro", CounterName::Chapter, bare);

    assert_eq!(r.resolve_label("ch:intro"), Some("Chapter 1"));
  }

  #[test]
  fn evaluate_ref_equation_uses_parenthesized_ref_format() {
    // 既定 equation は ref_format = "({number})" なので "(1.1)" が返る
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter);
    let bare = r.increment(CounterName::Equation);
    r.register_label("eq:foo", CounterName::Equation, bare);

    assert_eq!(r.resolve_label("eq:foo"), Some("(1.1)"));
  }

  #[test]
  fn evaluate_unknown_label_errors() {
    let r = CounterRegistry::default_for_seiran();
    assert!(r.resolve_label("nonexistent").is_none());
  }

  #[test]
  fn from_style_with_default_style_matches_default_for_seiran() {
    // Arrange / Act
    let mut from_default = CounterRegistry::from_style(&Style::default());
    let mut from_helper = CounterRegistry::default_for_seiran();

    // Assert: 既定 Style 経由と default_for_seiran() が同じ振る舞いをする
    assert_eq!(from_default.increment(CounterName::Chapter), from_helper.increment(CounterName::Chapter));
    assert_eq!(from_default.increment(CounterName::Section), from_helper.increment(CounterName::Section));
  }

  #[test]
  fn counter_name_for_heading_maps_each_level() {
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Part), CounterName::Part);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Chapter), CounterName::Chapter);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Subparagraph), CounterName::Subparagraph);
  }
}
