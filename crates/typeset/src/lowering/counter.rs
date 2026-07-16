//! 番号カウンタとラベル登録のレジストリ
//!
//! `\section`、`\begin{equation}`、`\begin{figure}` 等で発番される番号を一元管理し、
//! `\ref{label}` の解決に使うラベル → 番号の対応表を保持します。
//!
//! ## 2 パス lowering との関係
//!
//! - **pass1**: [`super::lower_nodes`] が `DocNode` ツリーを走査するたびに
//!   [`CounterRegistry::increment`] を呼び、`Heading.label` 等の任意ラベルが
//!   付いていれば [`CounterRegistry::register_label`] で登録する。
//! - **pass2**: [`super::resolve::resolve_refs`] が `LayoutNode::Ref { label, .. }` を
//!   [`CounterRegistry::resolve_label`] で解決し、`LayoutNode::Link` に書き換える。
//!
//! ## 番号書式について
//!
//! 各カウンタは [`config::CounterStyle`] の `format` テンプレート（例: `"{n}"`、
//! `"{chapter}.{n}"`）に従って文字列化される。`{n}` は自身のカウンタ値、`{<name>}` は
//! 他カウンタの値を参照先カウンタの [`config::NumberStyle`] でレンダリングする
//! （再帰展開はしない）。「3章」「第3部」のような装飾文字列は見出しの
//! `config::HeadingStyle.format` テンプレを介して付ける（`super::heading`）。
//!
//! ## `\ref` の書式について
//!
//! `\ref{label}` の表示は [`config::CounterStyle::ref_format`] テンプレートで決まる
//! （例: `"{display_name} {number}"` → `"Section 1.2"`、`"({number})"` → `"(1.2)"`）。
//! `register_label` 時点でテンプレートを適用するため、`resolve_label` は整形済み文字列を
//! 返す（呼び出し側は装飾を気にせず使える）。
//!
//! ## カウンタ定義のソース
//!
//! カウンタ定義の真のソースは `config::Style.counters` テーブル。
//! [`CounterRegistry::from_style`] が [`config::Counters`] を `defs` に複製し、
//! 実行時のカウンタ値は `HashMap<CounterName, u32>` で保持する（未登場のカウンタは 0）。

use std::collections::HashMap;

use config::{CounterName, Counters, Style, TheoremClass, TheoremReset, Theorems};
use model::HeadingLevel;

use super::{LoweringError, SourceId};

mod format;

use format::expand_ref_format;

/// カウンタ群の状態と labels の登録状態を保持するレジストリ
#[derive(Debug, Clone)]
pub(crate) struct CounterRegistry {
  /// カウンタ定義（`config::Counters` の複製）
  defs: Counters,
  /// 各カウンタの現在値。未登場のカウンタは 0 とみなす
  values: HashMap<CounterName, u32>,
  /// 定理クラス定義（`config::Theorems` の複製）。共有カウンタ名・リセット先・番号書式を引く
  theorems: Theorems,
  /// 定理カウンタの現在値。キーは共有カウンタ名（`TheoremStyle.counter`）。未登場は 0
  theorem_values: HashMap<String, u32>,
  /// `\ref` 解決用テーブル。pass1 で登録、pass2 で参照する
  labels: HashMap<String, ResolvedLabel>,
  /// 脚注カウンタの現在値（出現順の連番。ページ単位リセットは #35 で対応、現状は文書全体で連番）
  footnote_value: u32,
}

impl CounterRegistry {
  /// `config::Style` からレジストリを構築する
  ///
  /// カウンタ定義（9 種）に加え、定理クラス定義（`style.theorems`）も取り込む。
  #[must_use]
  pub(crate) fn from_style(style: &Style) -> Self {
    return Self {
      defs: style.counters.clone(),
      values: HashMap::new(),
      theorems: style.theorems.clone(),
      theorem_values: HashMap::new(),
      labels: HashMap::new(),
      footnote_value: 0,
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、書式化済みの番号文字列を返す
  ///
  /// 見出しカウンタ（part / chapter / section / subsection）の増加時には、その見出しレベルを
  /// `reset_by` に指定した定理カウンタも 0 に戻す（`reset_by` カウンタの「増加」に連動。LaTeX の
  /// `\newtheorem{thm}{Theorem}[section]` と同じく、上位カウンタのリセットでは戻さない）。
  pub(crate) fn increment(&mut self, name: CounterName) -> String {
    *self.values.entry(name).or_insert(0) += 1;
    for r in &self.defs.get(name).resets {
      self.values.insert(*r, 0);
    }
    if let Some(level) = theorem_reset_level(name) {
      self.reset_theorems_for_level(level);
    }

    return self.format_number(name);
  }

  /// 指定した見出しレベルを `reset_by` に持つ定理カウンタをすべて 0 に戻す
  fn reset_theorems_for_level(&mut self, level: TheoremReset) {
    // self.theorems への不変借用を先に解消してから theorem_values を変更するため、対象を収集する
    let to_reset: Vec<String> = self
      .theorems
      .iter_with_class()
      .filter(|(_, def)| return def.reset_by == level)
      .map(|(_, def)| return def.counter.clone())
      .collect();
    for counter in to_reset {
      self.theorem_values.insert(counter, 0);
    }
  }

  /// 定理環境を採番し、`label` があれば cleveref 形式で登録する
  ///
  /// クラスの共有カウンタ（`TheoremStyle.counter`）を 1 増やし、クラスの `number_format` テンプレート
  /// （`"{n}"` / `"{chapter}.{n}"` 等）で番号文字列を作って返す。`unnumbered` クラス（`proof`）は
  /// 採番せず `None` を返す。`label` が与えられた採番ありクラスでは `"{display_name} {number}"`
  /// （cleveref）で整形した文字列を `labels` に登録し、`\ref{label}` が「Theorem 1.2」等に解決される。
  ///
  /// # Errors
  ///
  /// `label` が既に登録済みの場合に [`LoweringError::DuplicateLabel`] を返します。
  pub(crate) fn increment_theorem_with_label(
    &mut self,
    class: TheoremClass,
    label: Option<&str>,
    span: model::Span,
    source: SourceId,
  ) -> Result<Option<String>, LoweringError> {
    // def への借用を必要なクローンに落としてから theorem_values を変更する
    let (counter, number_format, display_name, unnumbered) = {
      let def = self.theorems.get(class);
      (def.counter.clone(), def.number_format.clone(), def.display_name.clone(), def.unnumbered)
    };
    if unnumbered {
      return Ok(None);
    }

    let value = {
      let v = self.theorem_values.entry(counter).or_insert(0);
      *v += 1;
      *v
    };
    let number = self.expand_theorem_template(&number_format, value);

    if let Some(l) = label {
      let formatted = expand_ref_format("{display_name} {number}", &number, &display_name);
      if !self.register_formatted_label(l.to_string(), formatted) {
        return Err(LoweringError::DuplicateLabel {
          label: l.to_string(),
          span: super::span_to_source_span(span),
          source_id: source,
        });
      }
    }
    return Ok(Some(number));
  }

  /// 脚注を 1 つ採番し、番号を返す（出現順の連番。ラベル解決は不要なので単純増加のみ）
  ///
  /// ページ単位でのリセットは行わない（#35 の責務。改ページ情報は lowering 時点では未確定）。
  pub(crate) fn increment_footnote(&mut self) -> u32 {
    self.footnote_value += 1;
    return self.footnote_value;
  }

  /// 現在のカウンタ値を `number_format` テンプレートに従って書式化する
  #[must_use]
  pub(crate) fn format_number(&self, name: CounterName) -> String {
    return self.expand_template(&self.defs.get(name).number_format, name);
  }

  /// カウンタの現在値を返す（未登場のカウンタは 0）
  fn value(&self, name: CounterName) -> u32 { return self.values.get(&name).copied().unwrap_or(0); }

  /// テンプレートのプレースホルダ `{n}` / `{<counter_name>}` を値で置換する
  ///
  /// - `{n}` は `self_name` カウンタの値を、その `number_style` でレンダリングする
  /// - `{<name>}` は参照先カウンタの値を、参照先の `number_style` でレンダリングする
  ///   （テンプレートは再帰展開しない）
  /// - 未知のカウンタ名（9 種以外）は空文字列に置換する
  fn expand_template(&self, template: &str, self_name: CounterName) -> String {
    return super::placeholder::expand(template, |name| {
      let target = if name == "n" {
        Some(self_name)
      } else {
        CounterName::from_name(name)
      };
      return target.map_or_else(String::new, |t| return self.render_counter_value(t));
    });
  }

  /// 定理の番号テンプレート（`"{n}"` / `"{chapter}.{n}"` 等）を値で置換する
  ///
  /// - `{n}` は定理カウンタの現在値をアラビア数字で描画する（定理は `number_style` を持たない）
  /// - `{<name>}` は見出し等のカウンタ値を参照先の `number_style` でレンダリングする
  /// - 未知のプレースホルダは空文字列に置換する
  fn expand_theorem_template(&self, template: &str, self_value: u32) -> String {
    return super::placeholder::expand(template, |name| {
      if name == "n" {
        return self_value.to_string();
      }
      return CounterName::from_name(name).map_or_else(String::new, |t| return self.render_counter_value(t));
    });
  }

  /// カウンタの「現在値を自身の `number_style` で描画した文字列」を返す
  fn render_counter_value(&self, name: CounterName) -> String {
    return self.defs.get(name).number_style.render(self.value(name));
  }

  /// pass1 で `\section[label=sec:intro]{...}` などからラベルを登録する
  ///
  /// 渡された `number`（裸の番号）にカウンタの `ref_format` を適用し、`\ref` 時の表示
  /// 文字列を作って保存する（例: `"1.2"` → `"Section 1.2"`、`"({number})"` 形式なら `"(1.2)"`）。
  ///
  /// 同名ラベルが登録済みの場合は上書きせず `false` を返す。呼び出し側はこれを
  /// [`LoweringError::DuplicateLabel`] に変換して報告する（黙って上書きしない）。
  #[must_use]
  pub(crate) fn register_label(
    &mut self,
    label: impl Into<String>,
    counter: CounterName,
    number: impl Into<String>,
  ) -> bool {
    let label = label.into();
    if self.labels.contains_key(&label) {
      return false;
    }
    let def = self.defs.get(counter);
    let formatted = expand_ref_format(&def.ref_format, &number.into(), &def.display_name);
    self.labels.insert(label, ResolvedLabel { number: formatted });
    return true;
  }

  /// 整形済みの `\ref` 表示文字列をそのまま登録する（定理の cleveref 用）
  ///
  /// `register_label` がカウンタの `ref_format` を適用するのに対し、こちらは呼び出し側で
  /// 整形済みの文字列（例 `"Theorem 1.2"`）を受け取りそのまま保存する。定理クラスは
  /// `ref_format` フィールドを持たず、cleveref 書式（`"{display_name} {number}"`）を
  /// `increment_theorem_with_label` 側で適用するため、この経路を使う。
  ///
  /// 同名ラベルが登録済みの場合は上書きせず `false` を返す。
  #[must_use]
  fn register_formatted_label(&mut self, label: impl Into<String>, formatted: String) -> bool {
    let label = label.into();
    if self.labels.contains_key(&label) {
      return false;
    }
    self.labels.insert(label, ResolvedLabel { number: formatted });
    return true;
  }

  /// 採番とラベル登録を一括で行う共通処理
  ///
  /// [`CounterRegistry::increment`] で番号を発番し、`label` があれば
  /// [`CounterRegistry::register_label`] で登録する。同名ラベルが登録済みの場合は
  /// [`LoweringError::DuplicateLabel`] を返す。見出し・`equation`・`figure`・`table` の
  /// 各ハンドラが共用する。
  ///
  /// # Errors
  ///
  /// `label` が既に登録済みの場合に [`LoweringError::DuplicateLabel`] を返します。
  pub(crate) fn increment_with_label(
    &mut self,
    counter: CounterName,
    label: Option<&str>,
    span: model::Span,
    source: SourceId,
  ) -> Result<String, LoweringError> {
    let number = self.increment(counter);
    if let Some(l) = label
      && !self.register_label(l.to_string(), counter, &number)
    {
      return Err(LoweringError::DuplicateLabel {
        label: l.to_string(),
        span: super::span_to_source_span(span),
        source_id: source,
      });
    }
    return Ok(number);
  }

  /// pass2 で `\ref{label}` を解決して番号文字列を返す
  ///
  /// 未登録ラベルの場合は `None`。呼び出し側でエラー化する想定。
  #[must_use]
  pub(crate) fn resolve_label(&self, label: &str) -> Option<&str> {
    return self.labels.get(label).map(|r| return r.number.as_str());
  }

  /// 見出しレベルから seiran 既定の [`CounterName`] を返す
  #[must_use]
  pub(crate) fn counter_name_for_heading(level: HeadingLevel) -> CounterName {
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

#[cfg(test)]
impl CounterRegistry {
  /// seiran 既定のカウンタセットでレジストリを構築する
  ///
  /// 既定値は `config::Style::default()` が `Counters::default()` 経由で供給する
  /// 9 種（part / chapter / section / subsection / paragraph / subparagraph /
  /// figure / equation / table）。テスト用ショートカット。
  #[must_use]
  pub(crate) fn default_for_seiran() -> Self { return Self::from_style(&Style::default()); }

  /// `config::Counters` から直接レジストリを構築する（テスト・カスタム用）
  ///
  /// 定理クラス定義は [`Theorems::default`] を使う。定理カウンタを伴うテストでは
  /// [`CounterRegistry::from_style`] を使うこと。
  #[must_use]
  pub(crate) fn from_counters(counters: &Counters) -> Self {
    return Self {
      defs: counters.clone(),
      values: HashMap::new(),
      theorems: Theorems::default(),
      theorem_values: HashMap::new(),
      labels: HashMap::new(),
      footnote_value: 0,
    };
  }
}

/// pass1 で登録される、ラベル名から確定済み番号への解決結果
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLabel {
  /// 確定済みの `\ref` 表示文字列（カウンタは `ref_format`、定理は cleveref 書式を適用済み）
  number: String,
}

/// 見出しカウンタ [`CounterName`] を、定理カウンタの `reset_by` に対応する [`TheoremReset`] に写す
///
/// part / chapter / section / subsection の 4 種だけが定理リセットのトリガになりうる。
/// それ以外（paragraph / subparagraph / figure / equation / table）は `None` を返し、
/// 定理カウンタを巻き戻さない。
fn theorem_reset_level(name: CounterName) -> Option<TheoremReset> {
  return match name {
    CounterName::Part => Some(TheoremReset::Part),
    CounterName::Chapter => Some(TheoremReset::Chapter),
    CounterName::Section => Some(TheoremReset::Section),
    CounterName::Subsection => Some(TheoremReset::Subsection),
    _ => None,
  };
}

#[cfg(test)]
mod tests {
  use config::{CounterName, CounterStyle, Counters, NumberStyle, Style, TheoremClass, TheoremReset};
  use model::Span;

  use super::*;

  fn theorem_span() -> Span { return Span::DUMMY; }

  #[test]
  fn increment_theorem_numbers_with_default_format() {
    // Arrange — 既定 theorem は format "{n}"・counter "theorem"
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act / Assert
    assert_eq!(
      r.increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
        .unwrap()
        .as_deref(),
      Some("1")
    );
    assert_eq!(
      r.increment_theorem_with_label(TheoremClass::Lemma, None, theorem_span(), SourceId::new(0))
        .unwrap()
        .as_deref(),
      Some("2")
    );
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act / Assert — proof は採番なし
    assert!(
      r.increment_theorem_with_label(TheoremClass::Proof, None, theorem_span(), SourceId::new(0))
        .unwrap()
        .is_none()
    );
  }

  #[test]
  fn increment_theorem_definition_uses_separate_counter() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act
    let thm = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();
    let def = r
      .increment_theorem_with_label(TheoremClass::Definition, None, theorem_span(), SourceId::new(0))
      .unwrap();

    // Assert — 別カウンタなので両方 "1"
    assert_eq!(thm.as_deref(), Some("1"));
    assert_eq!(def.as_deref(), Some("1"));
  }

  #[test]
  fn theorem_format_embeds_section_counter() {
    // Arrange — theorem の number_format を "{section}.{n}" に変える
    let mut style = Style::default();
    style.theorems.theorem.number_format = "{section}.{n}".to_string();
    let mut r = CounterRegistry::from_style(&style);

    // Act — section を 1 に進めてから theorem を採番
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section);
    let number = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();

    // Assert — section=1、theorem=1 → "1.1"
    assert_eq!(number.as_deref(), Some("1.1"));
  }

  #[test]
  fn theorem_counter_resets_on_reset_by_heading() {
    // Arrange — theorem を reset_by=section・number_format "{section}.{n}" に設定
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = "{section}.{n}".to_string();
    let mut r = CounterRegistry::from_style(&style);
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section); // section = 1

    // Act
    let a = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();
    let b = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();
    r.increment(CounterName::Section); // section = 2、theorem カウンタは 0 にリセット
    let c = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();

    // Assert
    assert_eq!(a.as_deref(), Some("1.1"));
    assert_eq!(b.as_deref(), Some("1.2"));
    assert_eq!(c.as_deref(), Some("2.1"));
  }

  #[test]
  fn theorem_counter_not_reset_by_unrelated_heading() {
    // Arrange — reset_by=section の theorem は chapter の増加では戻らない（LaTeX と同じ）
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    let mut r = CounterRegistry::from_style(&style);
    r.increment(CounterName::Chapter); // chapter = 1

    // Act
    let a = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();
    r.increment(CounterName::Chapter); // chapter = 2（section は増えていない）
    let b = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .unwrap();

    // Assert — 連番が維持される
    assert_eq!(a.as_deref(), Some("1"));
    assert_eq!(b.as_deref(), Some("2"));
  }

  #[test]
  fn increment_theorem_registers_cleveref_label() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act
    r.increment_theorem_with_label(TheoremClass::Theorem, Some("thm:x"), theorem_span(), SourceId::new(0))
      .unwrap();

    // Assert — "{display_name} {number}" で解決される
    assert_eq!(r.resolve_label("thm:x"), Some("Theorem 1"));
  }

  #[test]
  fn increment_theorem_duplicate_label_errors() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());
    r.increment_theorem_with_label(TheoremClass::Theorem, Some("dup"), theorem_span(), SourceId::new(0))
      .unwrap();

    // Act
    let result = r.increment_theorem_with_label(TheoremClass::Lemma, Some("dup"), theorem_span(), SourceId::new(0));

    // Assert
    assert!(matches!(result, Err(LoweringError::DuplicateLabel { ref label, .. }) if label == "dup"));
  }

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
        number_format: "第{n}章".to_string(),
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
        number_format: "{n}".to_string(),
        number_style: NumberStyle::RomanUpper,
        ref_format: "{number}".to_string(),
        resets: vec![CounterName::Chapter],
      },
      chapter: CounterStyle {
        display_name: "Chapter".to_string(),
        number_format: "{part}-{n}".to_string(),
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
    assert!(r.register_label("ch:intro", CounterName::Chapter, bare));

    assert_eq!(r.resolve_label("ch:intro"), Some("Chapter 1"));
  }

  #[test]
  fn evaluate_ref_equation_uses_parenthesized_ref_format() {
    // 既定 equation は ref_format = "({number})" なので "(1.1)" が返る
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter);
    let bare = r.increment(CounterName::Equation);
    assert!(r.register_label("eq:foo", CounterName::Equation, bare));

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
  fn increment_footnote_returns_sequential_numbers() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();

    // Act / Assert
    assert_eq!(r.increment_footnote(), 1);
    assert_eq!(r.increment_footnote(), 2);
    assert_eq!(r.increment_footnote(), 3);
  }

  #[test]
  fn increment_footnote_unaffected_by_unrelated_counters() {
    // Arrange — section 等の見出しカウンタの増加は脚注カウンタに影響しない
    let mut r = CounterRegistry::default_for_seiran();
    r.increment_footnote(); // 1

    // Act
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section);
    let next = r.increment_footnote();

    // Assert
    assert_eq!(next, 2);
  }

  #[test]
  fn counter_name_for_heading_maps_each_level() {
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Part), CounterName::Part);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Chapter), CounterName::Chapter);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Subparagraph), CounterName::Subparagraph);
  }
}
