//! カウンタの値（構造のみ）と、ラベル・カウンタの登録状態を保持するレジストリ
//!
//! [`CounterValue`] は `resets` / `reset_by`（値に影響する style フィールド）だけから
//! 組み立てる。`number_format` 等の表示側フィールドはこのクレートが一切読まないことで、
//! G3（内容は見た目から独立）を型の設計として保証する。表示文字列の生成は typeset 側の
//! 責務（`typeset::lowering::counter`）。
//!
//! [`CounterRegistry`] は `typeset::lowering::counter::CounterRegistry`（issue #282 以前）から
//! 移設したもの。移設にあたり `increment` 系メソッドの戻り値を書式化済み `String` から
//! この構造値 [`CounterValue`] のみに変更し、`ref_format` 展開・`number_format` 展開などの
//! 表示生成コードは一切持ち込んでいない

use std::collections::HashMap;

#[cfg(test)]
use crate::style::{Counters, Style};
use crate::{
  document::{NodeId, SourceLocation, SourceMap, TheoremClass},
  semantics::{LabelId, SemanticError, SemanticPolicy},
  source::{SourceId, Span},
  style::{CounterName, TheoremReset},
};

/// カウンタの種別。`Counters`（見出し・図表・数式）と `Theorems`（定理クラス）の
/// 2 系統をひとつの型で表す
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CounterKind {
  /// `crate::style::Counters` が定義する固定 9 種のいずれか
  Counter(CounterName),
  /// 定理クラス（共有カウンタは `TheoremStyle.counter` で複数クラスが 1 つを共有しうる）
  Theorem(TheoremClass),
}

/// カウンタ値を構成する 1 要素 — どのカウンタの何番かの対
///
/// 名前は「どのカウンタか」という**構造**であって表示ではない（`display_name` /
/// `number_style` は持たない）。これを値に載せることで、表示側は祖先チェーンを
/// 再計算せずに `{chapter}` のような他カウンタ参照を名前で引ける
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CounterPart {
  /// この数がどのカウンタのものか
  pub name: CounterName,
  /// そのカウンタの値
  pub value: u32,
}

/// カウンタの値（構造のみ）。表示書式（`number_format` / `ref_format` / `number_style`）は
/// このクレートの対象外（typeset 側が `&crate::style::Style` と併せて表示文字列を作る）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterValue {
  /// このカウンタの種別
  pub kind: CounterKind,
  /// 祖先カウンタの値（最も遠い祖先から順・末尾が直近の親）
  pub ancestors: Vec<CounterPart>,
  /// このカウンタ自身の値
  pub own: u32,
}

impl CounterValue {
  /// `target` カウンタの値を返す（自身か祖先チェーン上にあるときだけ `Some`）
  ///
  /// 表示側（`typeset::lowering::counter`）が `{chapter}` のような他カウンタ参照を解決する
  /// 唯一の入口。値に載っていないカウンタ — 例えば `number_format = "{section}.{n}"` の図
  /// （既定では `section` は図の祖先ではない）— は復元できないので `None` を返す
  #[must_use]
  pub(crate) fn value_of(&self, target: CounterName) -> Option<u32> {
    if self.kind == CounterKind::Counter(target) {
      return Some(self.own);
    }
    return self.ancestors.iter().find(|part| return part.name == target).map(|part| return part.value);
  }
}

/// 走査中に登録される、ラベル名から確定済みカウンタ構造値への対応
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLabel {
  /// 登録時点のカウンタ構造値のスナップショット
  value: CounterValue,
  /// このラベルを定義した位置（重複時に「最初の定義」として示す）
  definition: SourceLocation,
}

/// カウンタ群の状態と labels の登録状態を保持するレジストリ
#[derive(Debug, Clone)]
pub(super) struct CounterRegistry {
  /// 意味解析が読む設定の投影（表示側フィールドは型として持たない）
  policy: SemanticPolicy,
  /// 各カウンタの現在値。未登場のカウンタは 0 とみなす
  values: HashMap<CounterName, u32>,
  /// 定理カウンタの現在値。キーは共有カウンタ名（`TheoremPolicy.counter`）。未登場は 0
  theorem_values: HashMap<String, u32>,
  /// `\ref` 解決用テーブル。走査中に登録し、走査後の参照の存在検証が引く
  labels: HashMap<LabelId, ResolvedLabel>,
}

impl CounterRegistry {
  /// `crate::semantics::SemanticPolicy` からレジストリを構築する
  #[must_use]
  pub(crate) fn from_policy(policy: &SemanticPolicy) -> Self {
    return Self {
      policy: policy.clone(),
      values: HashMap::new(),
      theorem_values: HashMap::new(),
      labels: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、構造値を返す
  pub(crate) fn increment(&mut self, name: CounterName) -> CounterValue {
    *self.values.entry(name).or_insert(0) += 1;
    for r in self.policy.counter(name).resets.clone() {
      self.values.insert(r, 0);
    }
    if let Some(level) = TheoremReset::for_counter(name) {
      self.reset_theorems_for_level(level);
    }

    return self.counter_value(name);
  }

  /// 指定した見出しレベルを `reset_by` に持つ定理カウンタをすべて 0 に戻す
  fn reset_theorems_for_level(&mut self, level: TheoremReset) {
    // self.policy への不変借用を先に解消してから theorem_values を変更するため、対象を収集する
    let to_reset: Vec<String> =
      self.policy.theorems_reset_by(level).map(|counter| return counter.to_string()).collect();
    for counter in to_reset {
      self.theorem_values.insert(counter, 0);
    }
  }

  /// 定理環境を採番し、`label` があれば構造値を登録する
  ///
  /// ラベルが既に登録済みなら [`SemanticError::DuplicateLabel`] を第 2 要素で返すが、**採番は
  /// 済んでいる**（重複は致命ではない）。最初の定義が有効なまま残り、呼び出し元は診断を
  /// 積んで走査を続けられる（#376）。
  pub(crate) fn increment_theorem_with_label(
    &mut self,
    class: TheoremClass,
    label: Option<&str>,
    span: Span,
    source_id: SourceId,
  ) -> (Option<CounterValue>, Option<SemanticError>) {
    // def への借用を必要なクローンに落としてから theorem_values を変更する
    let (counter, unnumbered) = {
      let def = self.policy.theorem(class);
      (def.counter.clone(), def.unnumbered)
    };
    if unnumbered {
      return (None, None);
    }

    *self.theorem_values.entry(counter).or_insert(0) += 1;
    let counter_value = self.theorem_counter_value(class);

    let Some(label) = label else {
      return (Some(counter_value), None);
    };
    let definition = SourceLocation { source_id, span };
    return match self.register_label(label.to_string(), counter_value.clone(), definition) {
      Ok(()) => (Some(counter_value), None),
      Err(first) => (Some(counter_value), Some(SemanticError::duplicate_label(label, definition, first))),
    };
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
    return CounterValue {
      kind: CounterKind::Counter(name),
      ancestors: self.ancestor_values(name),
      own: self.value(name),
    };
  }

  /// `name` の祖先カウンタの現在値を、最も遠い祖先から順に集める（末尾が直近の親）
  fn ancestor_values(&self, name: CounterName) -> Vec<CounterPart> {
    let own_index = CounterName::ALL
      .iter()
      .position(|candidate| return *candidate == name)
      .expect("CounterName::ALL は全 9 バリアントを含む");
    let parent = CounterName::ALL[..own_index]
      .iter()
      .rev()
      .find(|candidate| return self.policy.counter(**candidate).resets.contains(&name))
      .copied();
    let Some(parent) = parent else {
      return Vec::new();
    };
    let mut chain = self.ancestor_values(parent);
    chain.push(CounterPart {
      name: parent,
      value: self.value(parent),
    });
    return chain;
  }

  /// 定理クラスの現在値を、`reset_by` が指す見出しカウンタを祖先として [`CounterValue`] で返す
  #[must_use]
  pub(crate) fn theorem_counter_value(&self, class: TheoremClass) -> CounterValue {
    let def = self.policy.theorem(class);
    let own = *self.theorem_values.get(&def.counter).unwrap_or(&0);
    let ancestors = match def.reset_by.counter_name() {
      Some(heading_counter) => vec![CounterPart {
        name: heading_counter,
        value: self.value(heading_counter),
      }],
      None => Vec::new(),
    };
    return CounterValue {
      kind: CounterKind::Theorem(class),
      ancestors,
      own,
    };
  }

  /// 走査中に `\section[label=sec:intro]{...}` などからラベルを登録する
  ///
  /// 登録は先勝ち。同名のラベルが既にあれば登録せず、最初の定義位置を `Err` で返す。
  ///
  /// # Errors
  ///
  /// `label` が登録済みの場合に、最初に登録された定義位置を返す。
  pub(crate) fn register_label(
    &mut self,
    label: impl Into<LabelId>,
    value: CounterValue,
    definition: SourceLocation,
  ) -> Result<(), SourceLocation> {
    let label = label.into();
    if let Some(first) = self.labels.get(&label) {
      return Err(first.definition);
    }
    self.labels.insert(label, ResolvedLabel { value, definition });
    return Ok(());
  }

  /// 採番とラベル登録を一括で行う共通処理
  ///
  /// ラベルが既に登録済みなら [`SemanticError::DuplicateLabel`] を第 2 要素で返すが、**採番は
  /// 済んでいる**（重複は致命ではない）。最初の定義が有効なまま残り、呼び出し元は診断を
  /// 積んで走査を続けられる（#376）。
  pub(crate) fn increment_with_label(
    &mut self,
    counter: CounterName,
    label: Option<&str>,
    span: Span,
    source_id: SourceId,
  ) -> (CounterValue, Option<SemanticError>) {
    let value = self.increment(counter);
    let Some(label) = label else {
      return (value, None);
    };
    let definition = SourceLocation { source_id, span };
    return match self.register_label(label.to_string(), value.clone(), definition) {
      Ok(()) => (value, None),
      Err(first) => (value, Some(SemanticError::duplicate_label(label, definition, first))),
    };
  }

  /// 採番とラベル登録を一括で行う（HIR ノード版）
  ///
  /// 位置は `locations` から `node` を引いて求める。`(Span, Origin)` を直接受け取る
  /// [`Self::increment_with_label`] との使い分けは、呼び出し元が `NodeId` を持っているかどうか。
  pub(crate) fn increment_with_label_at(
    &mut self,
    counter: CounterName,
    label: Option<&str>,
    node: NodeId,
    locations: &SourceMap,
  ) -> (CounterValue, Option<SemanticError>) {
    let location = locations.location(node);
    return self.increment_with_label(counter, label, location.span, location.source_id);
  }

  /// 定理環境の採番とラベル登録を行う（HIR ノード版）
  pub(crate) fn increment_theorem_with_label_at(
    &mut self,
    class: TheoremClass,
    label: Option<&str>,
    node: NodeId,
    locations: &SourceMap,
  ) -> (Option<CounterValue>, Option<SemanticError>) {
    let location = locations.location(node);
    return self.increment_theorem_with_label(class, label, location.span, location.source_id);
  }

  /// 走査後の参照の存在検証で `\ref{label}` を解決し、カウンタの構造値（[`CounterValue`]）を返す
  #[must_use]
  pub(crate) fn resolve_label(&self, label: &str) -> Option<&CounterValue> {
    return self.labels.get(label).map(|r| return &r.value);
  }
}

#[cfg(test)]
impl CounterRegistry {
  /// seiran 既定のカウンタセットでレジストリを構築する
  #[must_use]
  pub(crate) fn default_for_seiran() -> Self {
    return Self::from_policy(&SemanticPolicy::from_style(&Style::default()));
  }

  /// `crate::style::Counters` から直接レジストリを構築する（テスト・カスタム用）
  #[must_use]
  pub(crate) fn from_counters(counters: &Counters) -> Self {
    let style = Style {
      counters: counters.clone(),
      ..Style::default()
    };
    return Self::from_policy(&SemanticPolicy::from_style(&style));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    source::SourceId,
    style::{CounterStyle, CounterTemplate, Counters, NumberStyle, ReferenceTemplate, Style, TheoremReset},
  };

  fn theorem_span() -> Span { return Span::DUMMY; }

  /// ソース `source` の `start` から 1 バイトのラベル定義位置を作る。
  fn location(source: usize, start: u32) -> SourceLocation {
    return SourceLocation {
      source_id: SourceId::new(source),
      span: Span::new(start, start + 1),
    };
  }

  /// 祖先チェーンを `(カウンタ名, 値)` の列にしてアサートしやすくする
  fn ancestors(value: &CounterValue) -> Vec<(CounterName, u32)> {
    return value.ancestors.iter().map(|part| return (part.name, part.value)).collect();
  }

  #[test]
  fn increment_theorem_numbers_with_default_style() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();

    // Act
    let thm = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .0
      .unwrap();
    let lemma = r
      .increment_theorem_with_label(TheoremClass::Lemma, None, theorem_span(), SourceId::new(0))
      .0
      .unwrap();

    // Assert
    assert_eq!(thm.own, 1);
    assert!(thm.ancestors.is_empty(), "既定の theorem は reset_by = none なので祖先なし");
    assert_eq!(lemma.own, 2, "既定では lemma が theorem とカウンタを共有する");
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();

    // Act
    let (value, duplicate) =
      r.increment_theorem_with_label(TheoremClass::Proof, None, theorem_span(), SourceId::new(0));

    // Assert
    assert!(value.is_none());
    assert!(duplicate.is_none());
  }

  #[test]
  fn increment_theorem_duplicate_label_reports_but_keeps_numbering() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment_theorem_with_label(TheoremClass::Theorem, Some("dup"), theorem_span(), SourceId::new(0));

    // Act
    let (value, duplicate) =
      r.increment_theorem_with_label(TheoremClass::Lemma, Some("dup"), theorem_span(), SourceId::new(0));

    // Assert — 重複は致命ではない（採番は済み、最初の定義が有効なまま残る）
    assert!(value.is_some(), "重複ラベルでも採番は行われるはず");
    assert!(matches!(duplicate, Some(SemanticError::DuplicateLabel { ref label, .. }) if label == "dup"));
    let Some(SemanticError::DuplicateLabel { labels, .. }) = duplicate else {
      panic!("DuplicateLabel を期待");
    };
    assert_eq!(labels.len(), 2, "同じソースの最初の定義も 2 本目のラベルとして示すはず: {labels:?}");
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
    assert_eq!(ancestors(&chapter), vec![(CounterName::Part, 0)], "part は未登場につき 0");
    assert_eq!(chapter.own, 1);
    assert_eq!(ancestors(&section_1), vec![(CounterName::Part, 0), (CounterName::Chapter, 1)]);
    assert_eq!(section_1.own, 1);
    assert_eq!(section_2.own, 2);
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
    assert_eq!(ancestors(&next), vec![(CounterName::Part, 0), (CounterName::Chapter, 2)]);
    assert_eq!(next.own, 1);
  }

  #[test]
  fn template_cross_counter_resets_via_config() {
    // Arrange
    let counters = Counters {
      part: CounterStyle {
        display_name: "Part".to_string(),
        number_format: CounterTemplate::parse("{n}"),
        number_style: NumberStyle::RomanUpper,
        ref_format: ReferenceTemplate::parse("{number}"),
        resets: vec![CounterName::Chapter],
      },
      chapter: CounterStyle {
        display_name: "Chapter".to_string(),
        number_format: CounterTemplate::parse("{part}-{n}"),
        number_style: NumberStyle::Arabic,
        ref_format: ReferenceTemplate::parse("{number}"),
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
    assert_eq!(ancestors(&ch), vec![(CounterName::Part, 2)], "part = 2");
    assert_eq!(ch.own, 1, "chapter = 1");
  }

  #[test]
  fn theorem_counter_resets_on_reset_by_heading() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    let mut r = CounterRegistry::from_policy(&SemanticPolicy::from_style(&style));
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section); // section = 1

    // Act
    let a = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .0
      .unwrap();
    let b = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .0
      .unwrap();
    r.increment(CounterName::Section); // section = 2、theorem カウンタは 0 にリセット
    let c = r
      .increment_theorem_with_label(TheoremClass::Theorem, None, theorem_span(), SourceId::new(0))
      .0
      .unwrap();

    // Assert
    assert_eq!(ancestors(&a), vec![(CounterName::Section, 1)], "祖先は reset_by が指す section だけ");
    assert_eq!(a.own, 1);
    assert_eq!(b.own, 2);
    assert_eq!(ancestors(&c), vec![(CounterName::Section, 2)]);
    assert_eq!(c.own, 1);
  }

  #[test]
  fn evaluate_unknown_label_returns_none() {
    let r = CounterRegistry::default_for_seiran();

    assert!(r.resolve_label("nonexistent").is_none());
  }

  #[test]
  fn resolve_label_returns_counter_value_snapshot() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter); // chapter = 1
    let value = r.increment(CounterName::Section); // section = 1
    r.register_label("sec:x", value, location(0, 0)).expect("初回の登録は成功するはず");

    // Act
    let resolved = r.resolve_label("sec:x").unwrap();

    // Assert
    assert_eq!(resolved.kind, CounterKind::Counter(CounterName::Section));
    assert_eq!(ancestors(resolved), vec![(CounterName::Part, 0), (CounterName::Chapter, 1)], "part → chapter の順");
    assert_eq!(resolved.own, 1);
  }

  #[test]
  fn counter_value_of_part_has_no_ancestor() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Part);

    // Act
    let value = r.increment(CounterName::Part);

    // Assert
    assert!(value.ancestors.is_empty(), "part を resets に含むカウンタは既定に無いので祖先なし");
    assert_eq!(value.own, 2);
  }

  #[test]
  fn register_label_rejects_duplicate_and_returns_the_first_definition() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    let value = r.increment(CounterName::Chapter);
    let first_site = location(0, 0);

    // Act
    let first = r.register_label("ch:intro", value.clone(), first_site);
    let second = r.register_label("ch:intro", value, location(1, 10));

    // Assert — 先勝ち。2 回目は登録されず、最初の定義位置が返る
    assert_eq!(first, Ok(()));
    assert_eq!(second, Err(first_site));
  }

  #[test]
  fn value_of_reads_own_and_ancestors_by_name() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Chapter);

    // Act
    let value = r.increment(CounterName::Section);

    // Assert
    assert_eq!(value.value_of(CounterName::Section), Some(1), "自身の値は kind から引ける");
    assert_eq!(value.value_of(CounterName::Chapter), Some(2), "祖先は名前で引ける");
    assert_eq!(value.value_of(CounterName::Figure), None, "値に載っていないカウンタは None");
  }
}
