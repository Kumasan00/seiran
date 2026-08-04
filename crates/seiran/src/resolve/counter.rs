//! カウンタの値（構造のみ）と、ラベル・カウンタの登録状態を保持するレジストリ
//!
//! [`CounterValue`] は `resets` / `reset_by`（値に影響する style フィールド）だけから
//! 組み立てる。`number_format` 等の表示側フィールドはこのクレートが一切読まないことで、
//! G3（内容は見た目から独立）を型の設計として保証する。表示文字列の生成は typeset 側の
//! 責務（`format_counter_value_for_style`、Task 7 で追加予定）。
//!
//! [`CounterRegistry`] は `typeset::lowering::counter::CounterRegistry`（issue #282 以前）から
//! 移設したもの。移設にあたり `increment` 系メソッドの戻り値を書式化済み `String` から
//! この構造値 [`CounterValue`] のみに変更し、`ref_format` 展開・`number_format` 展開などの
//! 表示生成コードは一切持ち込んでいない

use std::collections::HashMap;

use config::{CounterName, Counters, Style, TheoremReset, Theorems};
use model::{HeadingLevel, LabelId, Origin, Span, TheoremClass};

use crate::resolve::{ResolveError, error::span_to_source_span};

/// カウンタの種別。`Counters`（見出し・図表・数式）と `Theorems`（定理クラス）の
/// 2 系統をひとつの型で表す
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CounterKind {
  /// `config::Counters` が定義する固定 9 種のいずれか
  Counter(CounterName),
  /// 定理クラス（共有カウンタは `TheoremStyle.counter` で複数クラスが 1 つを共有しうる）
  Theorem(TheoremClass),
}

/// カウンタの値（構造のみ）。表示書式（`number_format` / `ref_format` / `number_style`）は
/// このクレートの対象外（typeset 側が `&config::Style` と併せて表示文字列を作る）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterValue {
  /// このカウンタの種別
  pub kind: CounterKind,
  /// 祖先カウンタから自身までの値列（祖先を辿った順。末尾が自身の値）
  pub parts: Vec<u32>,
}

/// pass1 で登録される、ラベル名から確定済みカウンタ構造値への対応
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLabel {
  /// 登録時点のカウンタ構造値のスナップショット
  value: CounterValue,
}

/// カウンタ群の状態と labels の登録状態を保持するレジストリ
#[derive(Debug, Clone)]
pub(crate) struct CounterRegistry {
  /// カウンタ定義（`config::Counters` の複製）
  defs: Counters,
  /// 各カウンタの現在値。未登場のカウンタは 0 とみなす
  values: HashMap<CounterName, u32>,
  /// 定理クラス定義（`config::Theorems` の複製）。共有カウンタ名・リセット先を引く
  theorems: Theorems,
  /// 定理カウンタの現在値。キーは共有カウンタ名（`TheoremStyle.counter`）。未登場は 0
  theorem_values: HashMap<String, u32>,
  /// `\ref` 解決用テーブル。pass1 で登録、pass2 で参照する
  labels: HashMap<LabelId, ResolvedLabel>,
}

impl CounterRegistry {
  /// `config::Style` からレジストリを構築する
  #[must_use]
  pub(crate) fn from_style(style: &Style) -> Self {
    return Self {
      defs: style.counters.clone(),
      values: HashMap::new(),
      theorems: style.theorems.clone(),
      theorem_values: HashMap::new(),
      labels: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、構造値を返す
  pub(crate) fn increment(&mut self, name: CounterName) -> CounterValue {
    *self.values.entry(name).or_insert(0) += 1;
    for r in &self.defs.get(name).resets {
      self.values.insert(*r, 0);
    }
    if let Some(level) = theorem_reset_level(name) {
      self.reset_theorems_for_level(level);
    }

    return self.counter_value(name);
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

  /// 定理環境を採番し、`label` があれば構造値を登録する
  ///
  /// # Errors
  ///
  /// `label` が既に登録済みの場合に [`ResolveError::DuplicateLabel`] を返します。
  pub(crate) fn increment_theorem_with_label(
    &mut self,
    class: TheoremClass,
    label: Option<&str>,
    span: Span,
    source: Origin,
  ) -> Result<Option<CounterValue>, ResolveError> {
    // def への借用を必要なクローンに落としてから theorem_values を変更する
    let (counter, unnumbered) = {
      let def = self.theorems.get(class);
      (def.counter.clone(), def.unnumbered)
    };
    if unnumbered {
      return Ok(None);
    }

    *self.theorem_values.entry(counter).or_insert(0) += 1;
    let counter_value = self.theorem_counter_value(class);

    if let Some(l) = label
      && !self.register_label(l.to_string(), counter_value.clone())
    {
      return Err(ResolveError::DuplicateLabel {
        label: l.to_string(),
        span: span_to_source_span(span),
        origin: source,
      });
    }
    return Ok(Some(counter_value));
  }

  /// カウンタの現在値を返す（未登場のカウンタは 0）
  fn value(&self, name: CounterName) -> u32 { return self.values.get(&name).copied().unwrap_or(0); }

  /// 指定カウンタの現在値を、祖先チェーンを辿って [`CounterValue`] として返す
  ///
  /// 表示側フィールド（`number_format` 等）は一切参照しない。祖先は「自分を `resets` に
  /// 含み、かつ `CounterName::ALL` の宣言順で自身より手前にあるカウンタのうち最も近いもの」
  /// を 1 段ずつ遡って求める（`resets` は値に影響する構造データであり、issue #282 の
  /// style 分類では「値側」に属する）。既定の `Counters` は祖先の `resets` に子孫を平坦に
  /// 列挙する（例: `part.resets` は `chapter` を含む）ため、探索範囲を「自身より手前」に
  /// 限定して最も近い候補を選ぶ。これにより祖先の飛び越え（`part` が `section` の直接の
  /// 親と誤認されること）を防ぎ、かつ候補の添字が再帰のたびに単調に減るため必ず停止する
  #[must_use]
  pub(crate) fn counter_value(&self, name: CounterName) -> CounterValue {
    let mut parts = self.ancestor_values(name);
    parts.push(self.value(name));
    return CounterValue {
      kind: CounterKind::Counter(name),
      parts,
    };
  }

  /// `name` の祖先カウンタの現在値を、最も遠い祖先から順に集める（末尾が直近の親）
  fn ancestor_values(&self, name: CounterName) -> Vec<u32> {
    let own_index = CounterName::ALL
      .iter()
      .position(|candidate| return *candidate == name)
      .expect("CounterName::ALL は全 9 バリアントを含む");
    let parent = CounterName::ALL[..own_index]
      .iter()
      .rev()
      .find(|candidate| return self.defs.get(**candidate).resets.contains(&name))
      .copied();
    let Some(parent) = parent else {
      return Vec::new();
    };
    let mut chain = self.ancestor_values(parent);
    chain.push(self.value(parent));
    return chain;
  }

  /// 定理クラスの現在値を、`reset_by` が指す見出しカウンタを祖先として [`CounterValue`] で返す
  #[must_use]
  pub(crate) fn theorem_counter_value(&self, class: TheoremClass) -> CounterValue {
    let def = self.theorems.get(class);
    let own = *self.theorem_values.get(&def.counter).unwrap_or(&0);
    let mut parts = match theorem_reset_counter_name(def.reset_by) {
      Some(heading_counter) => vec![self.value(heading_counter)],
      None => Vec::new(),
    };
    parts.push(own);
    return CounterValue {
      kind: CounterKind::Theorem(class),
      parts,
    };
  }

  /// pass1 で `\section[label=sec:intro]{...}` などからラベルを登録する
  #[must_use]
  pub(crate) fn register_label(&mut self, label: impl Into<LabelId>, value: CounterValue) -> bool {
    let label = label.into();
    if self.labels.contains_key(&label) {
      return false;
    }
    self.labels.insert(label, ResolvedLabel { value });
    return true;
  }

  /// 採番とラベル登録を一括で行う共通処理
  ///
  /// # Errors
  ///
  /// `label` が既に登録済みの場合に [`ResolveError::DuplicateLabel`] を返します。
  pub(crate) fn increment_with_label(
    &mut self,
    counter: CounterName,
    label: Option<&str>,
    span: Span,
    source: Origin,
  ) -> Result<CounterValue, ResolveError> {
    let value = self.increment(counter);
    if let Some(l) = label
      && !self.register_label(l.to_string(), value.clone())
    {
      return Err(ResolveError::DuplicateLabel {
        label: l.to_string(),
        span: span_to_source_span(span),
        origin: source,
      });
    }
    return Ok(value);
  }

  /// pass2 で `\ref{label}` を解決してカウンタの構造値（[`CounterValue`]）を返す
  #[must_use]
  pub(crate) fn resolve_label(&self, label: &str) -> Option<&CounterValue> {
    return self.labels.get(label).map(|r| return &r.value);
  }

  /// 登録済み全ラベルのカウンタ構造値を `HashMap<LabelId, CounterValue>` として取り出す
  ///
  /// `resolve_project` が最終的な
  /// [`crate::resolve::ResolvedDocument::counter_values`] を組み立てる際に使う。
  #[must_use]
  pub(crate) fn into_counter_values(self) -> HashMap<LabelId, CounterValue> {
    return self.labels.into_iter().map(|(label, resolved)| return (label, resolved.value)).collect();
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
  #[must_use]
  pub(crate) fn default_for_seiran() -> Self { return Self::from_style(&Style::default()); }

  /// `config::Counters` から直接レジストリを構築する（テスト・カスタム用）
  #[must_use]
  pub(crate) fn from_counters(counters: &Counters) -> Self {
    return Self {
      defs: counters.clone(),
      values: HashMap::new(),
      theorems: Theorems::default(),
      theorem_values: HashMap::new(),
      labels: HashMap::new(),
    };
  }
}

/// 見出しカウンタ [`CounterName`] を、定理カウンタの `reset_by` に対応する [`TheoremReset`] に写す
fn theorem_reset_level(name: CounterName) -> Option<TheoremReset> {
  return match name {
    CounterName::Part => Some(TheoremReset::Part),
    CounterName::Chapter => Some(TheoremReset::Chapter),
    CounterName::Section => Some(TheoremReset::Section),
    CounterName::Subsection => Some(TheoremReset::Subsection),
    _ => None,
  };
}

/// 定理の `reset_by`（見出しレベル）を、対応する見出しカウンタ [`CounterName`] に写す
fn theorem_reset_counter_name(reset_by: TheoremReset) -> Option<CounterName> {
  return match reset_by {
    TheoremReset::None => None,
    TheoremReset::Part => Some(CounterName::Part),
    TheoremReset::Chapter => Some(CounterName::Chapter),
    TheoremReset::Section => Some(CounterName::Section),
    TheoremReset::Subsection => Some(CounterName::Subsection),
  };
}

#[cfg(test)]
mod tests {
  use config::{CounterStyle, NumberStyle, TheoremReset};

  use super::*;

  fn theorem_span() -> Span { return Span::DUMMY; }

  #[test]
  fn increment_theorem_numbers_with_default_style() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act
    let thm = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap()
      .unwrap();
    let lemma = r
      .increment_theorem_with_label(TheoremClass::Lemma, None, theorem_span(), Origin::Source(model::SourceId::new(0)))
      .unwrap()
      .unwrap();

    // Assert
    assert_eq!(thm.parts, vec![1]);
    assert_eq!(lemma.parts, vec![2]);
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act
    let result = r
      .increment_theorem_with_label(TheoremClass::Proof, None, theorem_span(), Origin::Source(model::SourceId::new(0)))
      .unwrap();

    // Assert
    assert!(result.is_none());
  }

  #[test]
  fn increment_theorem_duplicate_label_errors() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());
    r.increment_theorem_with_label(
      TheoremClass::Theorem,
      Some("dup"),
      theorem_span(),
      Origin::Source(model::SourceId::new(0)),
    )
    .unwrap();

    // Act
    let result = r.increment_theorem_with_label(
      TheoremClass::Lemma,
      Some("dup"),
      theorem_span(),
      Origin::Source(model::SourceId::new(0)),
    );

    // Assert
    assert!(matches!(result, Err(ResolveError::DuplicateLabel { ref label, .. }) if label == "dup"));
  }

  #[test]
  fn counter_registry_increment_builds_ancestor_chain() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();

    // Act
    let chapter = r.increment(CounterName::Chapter);
    let section_1 = r.increment(CounterName::Section);
    let section_2 = r.increment(CounterName::Section);

    // Assert
    assert_eq!(chapter.parts, vec![0, 1], "part（未登場につき 0）→ chapter");
    assert_eq!(section_1.parts, vec![0, 1, 1], "part → chapter → section");
    assert_eq!(section_2.parts, vec![0, 1, 2]);
  }

  #[test]
  fn counter_registry_section_reset_on_chapter_increment() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter); // chapter = 1
    r.increment(CounterName::Section); // section = 1
    r.increment(CounterName::Section); // section = 2
    r.increment(CounterName::Chapter); // chapter = 2、section は 0 にリセット

    // Act
    let next = r.increment(CounterName::Section);

    // Assert
    assert_eq!(next.parts, vec![0, 2, 1]);
  }

  #[test]
  fn template_cross_counter_resets_via_config() {
    // Arrange
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

    // Assert
    assert_eq!(ch.parts, vec![2, 1], "part = 2、chapter = 1");
  }

  #[test]
  fn theorem_counter_resets_on_reset_by_heading() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    let mut r = CounterRegistry::from_style(&style);
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section); // section = 1

    // Act
    let a = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap()
      .unwrap();
    let b = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap()
      .unwrap();
    r.increment(CounterName::Section); // section = 2、theorem カウンタは 0 にリセット
    let c = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap()
      .unwrap();

    // Assert
    assert_eq!(a.parts, vec![1, 1]);
    assert_eq!(b.parts, vec![1, 2]);
    assert_eq!(c.parts, vec![2, 1]);
  }

  #[test]
  fn evaluate_unknown_label_returns_none() {
    // Arrange
    let r = CounterRegistry::default_for_seiran();

    // Act / Assert
    assert!(r.resolve_label("nonexistent").is_none());
  }

  #[test]
  fn resolve_label_returns_counter_value_snapshot() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter); // chapter = 1
    let value = r.increment(CounterName::Section); // section = 1
    assert!(r.register_label("sec:x", value));

    // Act
    let resolved = r.resolve_label("sec:x").unwrap();

    // Assert
    assert_eq!(resolved.kind, CounterKind::Counter(CounterName::Section));
    assert_eq!(resolved.parts, vec![0, 1, 1], "part（未登場につき 0）→ chapter → section の順");
  }

  #[test]
  fn counter_value_of_part_has_no_ancestor() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Part);

    // Act
    let value = r.increment(CounterName::Part);

    // Assert
    assert_eq!(value.parts, vec![2], "part を resets に含むカウンタは既定に無いので祖先なし");
  }

  #[test]
  fn counter_name_for_heading_maps_each_level() {
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Part), CounterName::Part);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Chapter), CounterName::Chapter);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Subparagraph), CounterName::Subparagraph);
  }

  #[test]
  fn register_label_rejects_duplicate() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    let value = r.increment(CounterName::Chapter);

    // Act
    let first = r.register_label("ch:intro", value.clone());
    let second = r.register_label("ch:intro", value);

    // Assert
    assert!(first);
    assert!(!second);
  }
}
