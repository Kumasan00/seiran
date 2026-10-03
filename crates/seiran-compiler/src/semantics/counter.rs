//! カウンタの値（構造のみ）と、カウンタの現在値を保持するレジストリ
//!
//! 祖先カウンタの値は [`CounterAncestor`] として「どのカウンタの何番か」を名前付きで運ぶ。**祖先の
//! 決め方を持つのは crate 内でこの module だけ**で、表示側は受け取った値を名前で引くだけになる。

use std::collections::HashMap;

use strum::VariantArray;

use crate::{document::TheoremClass, semantics::SemanticPolicy, style::CounterName};

/// カウンタの種別。`CounterStyles`（見出し・図表・数式）と `TheoremStyles`（定理クラス）の
/// 2 系統をひとつの型で表す
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CounterKind {
  /// `crate::style::CounterStyles` が定義する固定 9 種のいずれか
  Counter(CounterName),
  /// 定理クラス（共有カウンタは `TheoremStyle.counter` で複数クラスが 1 つを共有しうる）
  Theorem(TheoremClass),
}

/// 祖先カウンタ 1 つ分の値 — どのカウンタの何番かの対
///
/// 名前は「どのカウンタか」という**構造**であって表示ではない（`display_name` /
/// `number_style` は持たない）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CounterAncestor {
  /// この数がどのカウンタのものか
  pub name: CounterName,
  /// そのカウンタの値
  pub value: u32,
}

/// カウンタの値（構造のみ）
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CounterValue {
  /// このカウンタの種別
  pub kind: CounterKind,
  /// 祖先カウンタの値（最も遠い祖先から順・末尾が直近の親）
  pub ancestors: Vec<CounterAncestor>,
  /// このカウンタ自身の値
  pub own: u32,
}

impl CounterValue {
  /// `target` カウンタの値を返す（自身か祖先チェーン上にあるときだけ `Some`）
  ///
  /// 値に載っていないカウンタ — 例えば `number_format = "{section}.{n}"` の図
  /// （既定では `section` は図の祖先ではない）— は復元できないので `None` を返す
  #[must_use]
  pub(crate) fn value_of(&self, target: CounterName) -> Option<u32> {
    if self.kind == CounterKind::Counter(target) {
      return Some(self.own);
    }
    return self
      .ancestors
      .iter()
      .find(|ancestor| return ancestor.name == target)
      .map(|ancestor| return ancestor.value);
  }
}

/// カウンタ群の状態を保持するレジストリ
#[derive(Debug)]
pub(super) struct CounterRegistry<'p> {
  /// 意味解析が読む設定の投影
  policy: &'p SemanticPolicy,
  /// 各カウンタの現在値。未登場のカウンタは 0 とみなす
  values: HashMap<CounterName, u32>,
  /// 定理カウンタの現在値。キーは `policy` から借りた共有カウンタ名（`TheoremPolicy.counter`）で、
  /// 同じ名前を持つ別クラスは文字列の等値で同じ項目に当たる。未登場は 0
  theorem_values: HashMap<&'p str, u32>,
}

impl<'p> CounterRegistry<'p> {
  /// `crate::semantics::SemanticPolicy` を借用してレジストリを構築する
  #[must_use]
  pub(super) fn from_policy(policy: &'p SemanticPolicy) -> Self {
    return Self {
      policy,
      values: HashMap::new(),
      theorem_values: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、構造値を返す
  pub(super) fn increment(&mut self, name: CounterName) -> CounterValue {
    *self.values.entry(name).or_insert(0) += 1;
    for &r in &self.policy.counter(name).resets {
      self.values.insert(r, 0);
    }
    for counter in &self.policy.counter(name).theorem_resets {
      self.theorem_values.insert(counter.as_str(), 0);
    }

    return self.counter_value(name);
  }

  /// 定理環境を採番し、構造値を返す（無採番クラス（`proof`）は `None`）
  pub(super) fn increment_theorem(&mut self, class: TheoremClass) -> Option<CounterValue> {
    let def = self.policy.theorem(class);
    if def.unnumbered {
      return None;
    }
    *self.theorem_values.entry(def.counter.as_str()).or_insert(0) += 1;
    return Some(self.theorem_counter_value(class));
  }

  /// カウンタの現在値を返す（未登場のカウンタは 0）
  fn value(&self, name: CounterName) -> u32 { return self.values.get(&name).copied().unwrap_or(0); }

  /// 指定カウンタの現在値を、祖先チェーンを辿って [`CounterValue`] として返す
  #[must_use]
  fn counter_value(&self, name: CounterName) -> CounterValue {
    return CounterValue {
      kind: CounterKind::Counter(name),
      ancestors: self.ancestor_values(name),
      own: self.value(name),
    };
  }

  /// `name` の祖先カウンタの現在値を、最も遠い祖先から順に集める（末尾が直近の親）
  ///
  /// 祖先は「自分を `resets` に含み、かつ `CounterName::VARIANTS` の宣言順で自身より手前にある
  /// カウンタのうち最も近いもの」を 1 段ずつ遡って求める。既定の `CounterStyles` は祖先の `resets` に
  /// 子孫を平坦に列挙する（例: `part.resets` は `chapter` を含む）ため、探索範囲を「自身より手前」に
  /// 限定して最も近い候補を選ぶ。これにより祖先の飛び越え（`part` が `section` の直接の
  /// 親と誤認されること）を防ぎ、かつ候補の添字が再帰のたびに単調に減るため必ず停止する
  fn ancestor_values(&self, name: CounterName) -> Vec<CounterAncestor> {
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
    chain.push(CounterAncestor {
      name: parent,
      value: self.value(parent),
    });
    return chain;
  }

  /// 定理クラスの現在値を、`reset_by` が指す見出しカウンタを祖先として [`CounterValue`] で返す
  #[must_use]
  fn theorem_counter_value(&self, class: TheoremClass) -> CounterValue {
    let def = self.policy.theorem(class);
    let own = *self.theorem_values.get(def.counter.as_str()).unwrap_or(&0);
    let ancestors = match def.reset_by {
      Some(heading_counter) => vec![CounterAncestor {
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
  use crate::style::{Style, TheoremReset};

  /// seiran 既定のスタイルから意味解析の投影を作る
  fn default_policy() -> SemanticPolicy { return SemanticPolicy::from_style(&Style::default()); }

  /// 祖先チェーンを `(カウンタ名, 値)` の列にしてアサートしやすくする
  fn ancestors(value: &CounterValue) -> Vec<(CounterName, u32)> {
    return value.ancestors.iter().map(|ancestor| return (ancestor.name, ancestor.value)).collect();
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);
    let value = r.increment_theorem(TheoremClass::Proof);
    assert!(value.is_none());
  }

  #[test]
  fn counter_registry_increment_builds_ancestor_chain() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);

    let chapter = r.increment(CounterName::Chapter);
    let section_1 = r.increment(CounterName::Section);
    let section_2 = r.increment(CounterName::Section);

    assert_eq!(ancestors(&chapter), vec![(CounterName::Part, 0)], "part は未登場につき 0");
    assert_eq!(chapter.own, 1);
    assert_eq!(ancestors(&section_1), vec![(CounterName::Part, 0), (CounterName::Chapter, 1)]);
    assert_eq!(section_1.own, 1);
    assert_eq!(ancestors(&section_2), vec![(CounterName::Part, 0), (CounterName::Chapter, 1)]);
    assert_eq!(section_2.own, 2);
  }

  #[test]
  fn counter_registry_section_reset_on_chapter_increment() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);
    r.increment(CounterName::Chapter); // chapter = 1
    r.increment(CounterName::Section); // section = 1
    r.increment(CounterName::Section); // section = 2
    r.increment(CounterName::Chapter); // chapter = 2、section は 0 にリセット

    let next = r.increment(CounterName::Section);

    assert_eq!(ancestors(&next), vec![(CounterName::Part, 0), (CounterName::Chapter, 2)]);
    assert_eq!(next.own, 1);
  }

  #[test]
  fn theorem_counter_resets_on_reset_by_heading() {
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    let policy = SemanticPolicy::from_style(&style);
    let mut r = CounterRegistry::from_policy(&policy);
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section); // section = 1

    let a = r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず");
    let b = r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず");
    r.increment(CounterName::Section); // section = 2、theorem カウンタは 0 にリセット
    let c = r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず");

    assert_eq!(ancestors(&a), vec![(CounterName::Section, 1)], "祖先は reset_by が指す section だけ");
    assert_eq!(a.own, 1);
    assert_eq!(b.own, 2);
    assert_eq!(ancestors(&c), vec![(CounterName::Section, 2)]);
    assert_eq!(c.own, 1);
  }

  #[test]
  fn shared_theorem_counter_resets_for_every_sharing_class() {
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    let policy = SemanticPolicy::from_style(&style);
    let mut r = CounterRegistry::from_policy(&policy);
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section); // section = 1
    r.increment_theorem(TheoremClass::Theorem).expect("採番されるはず"); // 共有カウンタ theorem = 1
    let before = r.increment_theorem(TheoremClass::Lemma).expect("採番されるはず"); // theorem = 2

    r.increment(CounterName::Section); // theorem クラスの reset_by で共有カウンタ theorem が 0 に戻る
    let after = r.increment_theorem(TheoremClass::Lemma).expect("採番されるはず");

    assert_eq!(before.own, 2, "lemma は既定で theorem とカウンタを共有する");
    assert_eq!(after.own, 1, "theorem クラスの reset_by が共有カウンタを戻し、lemma の番号にも効く");
    assert!(after.ancestors.is_empty(), "lemma 自身は reset_by = none なので祖先なし");
  }

  #[test]
  fn value_of_reads_own_and_ancestors_by_name() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Chapter);

    let value = r.increment(CounterName::Section);

    assert_eq!(value.value_of(CounterName::Section), Some(1), "自身の値は kind から引ける");
    assert_eq!(value.value_of(CounterName::Chapter), Some(2), "祖先は名前で引ける");
    assert_eq!(value.value_of(CounterName::Figure), None, "値に載っていないカウンタは None");
  }
}
