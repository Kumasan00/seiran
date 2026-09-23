//! カウンタの値（構造のみ）と、カウンタの現在値を保持するレジストリ
//!
//! [`CounterValue`] は `resets` / `reset_by`（値に影響する style フィールド）だけから
//! 組み立てる。`number_format` 等の表示側フィールドはこのクレートが一切読まないことで、
//! G3（内容は見た目から独立）を型の設計として保証する。表示文字列の生成は typeset 側の
//! 責務（`typeset::lowering::counter`）。
//!
//! 値の各要素は [`CounterPart`] として「どのカウンタの何番か」を名前付きで運ぶ。名前は
//! 構造であって表示ではないので、値と表示の分離（#282）と矛盾しない。**祖先の決め方を
//! 持つのは crate 内でこの module だけ**で、表示側は受け取った値を名前で引くだけになる
//! （#665）。
//!
//! [`CounterRegistry`] は `typeset::lowering::counter::CounterRegistry`（issue #282 以前）から
//! 移設したもの。移設にあたり `increment` 系メソッドの戻り値を書式化済み `String` から
//! この構造値 [`CounterValue`] のみに変更し、`ref_format` 展開・`number_format` 展開などの
//! 表示生成コードは一切持ち込んでいない
//!
//! ラベルの定義表は持たない — ラベルは意味の事実なので `semantics::facts` の 1 表が先勝ちで
//! 持ち、レジストリは採番だけを担う（#666）。

use std::collections::HashMap;

use strum::VariantArray;

use crate::{
  document::TheoremClass,
  semantics::SemanticPolicy,
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
#[derive(Debug, PartialEq, Eq)]
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

/// カウンタ群の状態を保持するレジストリ
#[derive(Debug)]
pub(super) struct CounterRegistry {
  /// 意味解析が読む設定の投影（表示側フィールドは型として持たない）
  policy: SemanticPolicy,
  /// 各カウンタの現在値。未登場のカウンタは 0 とみなす
  values: HashMap<CounterName, u32>,
  /// 定理カウンタの現在値。キーは共有カウンタ名（`TheoremPolicy.counter`）。未登場は 0
  theorem_values: HashMap<String, u32>,
}

impl CounterRegistry {
  /// `crate::semantics::SemanticPolicy` からレジストリを構築する
  #[must_use]
  pub(super) fn from_policy(policy: &SemanticPolicy) -> Self {
    return Self {
      policy: policy.clone(),
      values: HashMap::new(),
      theorem_values: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、構造値を返す
  pub(super) fn increment(&mut self, name: CounterName) -> CounterValue {
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

  /// 定理環境を採番し、構造値を返す（無採番クラス（`proof`）は `None`）
  pub(super) fn increment_theorem(&mut self, class: TheoremClass) -> Option<CounterValue> {
    // def への借用を必要なクローンに落としてから theorem_values を変更する
    let (counter, unnumbered) = {
      let def = self.policy.theorem(class);
      (def.counter.clone(), def.unnumbered)
    };
    if unnumbered {
      return None;
    }
    *self.theorem_values.entry(counter).or_insert(0) += 1;
    return Some(self.theorem_counter_value(class));
  }

  /// カウンタの現在値を返す（未登場のカウンタは 0）
  fn value(&self, name: CounterName) -> u32 { return self.values.get(&name).copied().unwrap_or(0); }

  /// 指定カウンタの現在値を、祖先チェーンを辿って [`CounterValue`] として返す
  ///
  /// 表示側フィールド（`number_format` 等）は一切参照しない。祖先は「自分を `resets` に
  /// 含み、かつ `CounterName::VARIANTS` の宣言順で自身より手前にあるカウンタのうち最も近いもの」
  /// を 1 段ずつ遡って求める（`resets` は値に影響する構造データであり、issue #282 の
  /// style 分類では「値側」に属する）。既定の `Counters` は祖先の `resets` に子孫を平坦に
  /// 列挙する（例: `part.resets` は `chapter` を含む）ため、探索範囲を「自身より手前」に
  /// 限定して最も近い候補を選ぶ。これにより祖先の飛び越え（`part` が `section` の直接の
  /// 親と誤認されること）を防ぎ、かつ候補の添字が再帰のたびに単調に減るため必ず停止する
  #[must_use]
  fn counter_value(&self, name: CounterName) -> CounterValue {
    return CounterValue {
      kind: CounterKind::Counter(name),
      ancestors: self.ancestor_values(name),
      own: self.value(name),
    };
  }

  /// `name` の祖先カウンタの現在値を、最も遠い祖先から順に集める（末尾が直近の親）
  fn ancestor_values(&self, name: CounterName) -> Vec<CounterPart> {
    let own_index = CounterName::VARIANTS
      .iter()
      .position(|candidate| return *candidate == name)
      .expect("CounterName::VARIANTS は全 9 バリアントを含む");
    let parent = CounterName::VARIANTS[..own_index]
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
  fn theorem_counter_value(&self, class: TheoremClass) -> CounterValue {
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
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::style::{CounterStyle, CounterTemplate, Counters, NumberStyle, ReferenceTemplate, Style, TheoremReset};

  impl CounterRegistry {
    /// seiran 既定のカウンタセットでレジストリを構築する
    #[must_use]
    fn default_for_seiran() -> Self { return Self::from_policy(&SemanticPolicy::from_style(&Style::default())); }

    /// `crate::style::Counters` から直接レジストリを構築する（テスト・カスタム用）
    #[must_use]
    fn from_counters(counters: &Counters) -> Self {
      let style = Style {
        counters: counters.clone(),
        ..Style::default()
      };
      return Self::from_policy(&SemanticPolicy::from_style(&style));
    }
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
    let thm = r.increment_theorem(TheoremClass::Theorem).expect("既定の theorem は採番されるはず");
    let lemma = r.increment_theorem(TheoremClass::Lemma).expect("既定の lemma は採番されるはず");

    // Assert
    assert_eq!(thm.own, 1);
    assert!(thm.ancestors.is_empty(), "既定の theorem は reset_by = none なので祖先なし");
    assert_eq!(lemma.own, 2, "既定では lemma が theorem とカウンタを共有する");
    assert!(lemma.ancestors.is_empty(), "既定の lemma も reset_by = none なので祖先なし");
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();

    // Act
    let value = r.increment_theorem(TheoremClass::Proof);

    // Assert
    assert!(value.is_none());
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
    assert_eq!(ancestors(&section_2), vec![(CounterName::Part, 0), (CounterName::Chapter, 1)]);
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
    let a = r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず");
    let b = r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず");
    r.increment(CounterName::Section); // section = 2、theorem カウンタは 0 にリセット
    let c = r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず");

    // Assert
    assert_eq!(ancestors(&a), vec![(CounterName::Section, 1)], "祖先は reset_by が指す section だけ");
    assert_eq!(a.own, 1);
    assert_eq!(b.own, 2);
    assert_eq!(ancestors(&c), vec![(CounterName::Section, 2)]);
    assert_eq!(c.own, 1);
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
