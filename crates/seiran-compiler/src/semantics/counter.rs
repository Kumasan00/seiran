//! カウンタの値（構造のみ）と、カウンタの現在値を保持するレジストリ
//!
//! 値は採番時点の全カウンタの値を名前で引ける形で運ぶ。番号書式がどのカウンタを参照しても表示側は
//! 名前で引くだけなので、`resets` / `reset_by` は採番時のリセットにだけ効き、表示できる値を制限しない。

use std::{
  collections::HashMap,
  ops::{Index, IndexMut},
};

use strum::VariantArray;

use crate::{document::TheoremClass, semantics::SemanticPolicy, style::CounterName};

/// カウンタの数（[`CounterValues`] のスロット数）
const SLOTS: usize = CounterName::VARIANTS.len();

// `CounterValues` は判別子をそのまま配列の添字に使うので、ここが破れると別のカウンタの値を返してしまう。
const _: () = {
  let mut index = 0;
  while index < SLOTS {
    assert!(
      CounterName::VARIANTS[index] as usize == index,
      "CounterName::VARIANTS は宣言順に並んでいなければならない"
    );
    index += 1;
  }
};

/// 固定 9 種のカウンタそれぞれの値（未登場のカウンタは 0）
///
/// 値は `[u32; 9]` に [`CounterName`] の宣言順で並ぶので、全カウンタの値が揃っていることは型が保証する。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CounterValues {
  /// [`CounterName`] の宣言順に並んだカウンタごとの値
  values: [u32; SLOTS],
}

impl Index<CounterName> for CounterValues {
  type Output = u32;

  // 添字は判別子。module 先頭の const 検査が `CounterName::VARIANTS` と判別子の順序の一致を保証する。
  fn index(&self, name: CounterName) -> &u32 { return &self.values[name as usize]; }
}

impl IndexMut<CounterName> for CounterValues {
  fn index_mut(&mut self, name: CounterName) -> &mut u32 { return &mut self.values[name as usize]; }
}

/// 採番された 1 項目の値（構造のみ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterValue {
  /// 固定 9 種のカウンタのいずれかで採番された項目（見出し・図・表・数式）
  Counter {
    /// 採番したカウンタ。自身の値は `counters[name]`
    name: CounterName,
    /// 採番時点（自身の加算とリセットの後）の全カウンタの値
    counters: CounterValues,
  },
  /// 定理クラスで採番された項目
  Theorem {
    /// 定理クラス（共有カウンタは `TheoremStyle.counter` で複数クラスが 1 つを共有しうる）
    class: TheoremClass,
    /// 定理の共有カウンタの値
    own: u32,
    /// 採番時点の全カウンタの値
    counters: CounterValues,
  },
}

/// カウンタ群の状態を保持するレジストリ
#[derive(Debug)]
pub(super) struct CounterRegistry<'p> {
  /// 意味解析が読む設定の投影
  policy: &'p SemanticPolicy,
  /// 各カウンタの現在値
  values: CounterValues,
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
      values: CounterValues::default(),
      theorem_values: HashMap::new(),
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、構造値を返す
  pub(super) fn increment(&mut self, name: CounterName) -> CounterValue {
    self.values[name] += 1;
    for &r in &self.policy.counter(name).resets {
      self.values[r] = 0;
    }
    for counter in &self.policy.counter(name).theorem_resets {
      self.theorem_values.insert(counter.as_str(), 0);
    }

    return CounterValue::Counter {
      name,
      counters: self.values,
    };
  }

  /// 定理環境を採番し、構造値を返す（無採番クラス（`proof`）は `None`）
  pub(super) fn increment_theorem(&mut self, class: TheoremClass) -> Option<CounterValue> {
    let def = self.policy.theorem(class);
    if def.unnumbered {
      return None;
    }
    let own = self.theorem_values.entry(def.counter.as_str()).or_insert(0);
    *own += 1;
    return Some(CounterValue::Theorem {
      class,
      own: *own,
      counters: self.values,
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::style::{Style, TheoremReset};

  /// seiran 既定のスタイルから意味解析の投影を作る
  fn default_policy() -> SemanticPolicy { return SemanticPolicy::from_style(&Style::default()); }

  /// 構造値に載った全カウンタの値
  fn counters(value: &CounterValue) -> CounterValues {
    return match value {
      CounterValue::Counter { counters, .. } | CounterValue::Theorem { counters, .. } => *counters,
    };
  }

  /// 定理の構造値の共有カウンタの値
  fn theorem_own(value: &CounterValue) -> u32 {
    let CounterValue::Theorem { own, .. } = value else {
      panic!("定理の構造値のはず: {value:?}");
    };
    return *own;
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);
    let value = r.increment_theorem(TheoremClass::Proof);
    assert!(value.is_none());
  }

  #[test]
  fn counter_registry_increment_snapshots_every_counter() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);

    let chapter = r.increment(CounterName::Chapter);
    let section_1 = r.increment(CounterName::Section);
    let section_2 = r.increment(CounterName::Section);

    assert!(matches!(
      chapter,
      CounterValue::Counter {
        name: CounterName::Chapter,
        ..
      }
    ));
    assert_eq!(counters(&chapter)[CounterName::Chapter], 1);
    assert_eq!(counters(&chapter)[CounterName::Part], 0, "未登場の part は 0");
    assert_eq!(counters(&section_1)[CounterName::Chapter], 1);
    assert_eq!(counters(&section_1)[CounterName::Section], 1);
    assert_eq!(counters(&section_2)[CounterName::Section], 2);
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

    assert_eq!(counters(&next)[CounterName::Chapter], 2);
    assert_eq!(counters(&next)[CounterName::Section], 1);
  }

  #[test]
  fn snapshot_carries_counters_outside_the_reset_relation() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section);
    r.increment(CounterName::Section); // section = 2

    // 既定では section は figure をリセットしないが、採番時点の値は載る
    let figure = r.increment(CounterName::Figure);

    assert_eq!(counters(&figure)[CounterName::Section], 2);
    assert_eq!(counters(&figure)[CounterName::Figure], 1);
  }

  #[test]
  fn snapshot_is_not_rewritten_by_later_increments() {
    let policy = default_policy();
    let mut r = CounterRegistry::from_policy(&policy);
    r.increment(CounterName::Chapter);
    let section = r.increment(CounterName::Section);

    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section);

    assert_eq!(counters(&section)[CounterName::Chapter], 1, "採番後に章が進んでも採番時点の値のまま");
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

    assert_eq!(counters(&a)[CounterName::Chapter], 1, "reset_by 以外の見出しカウンタも載る");
    assert_eq!(counters(&a)[CounterName::Section], 1);
    assert_eq!(theorem_own(&a), 1);
    assert_eq!(theorem_own(&b), 2);
    assert_eq!(counters(&c)[CounterName::Section], 2);
    assert_eq!(theorem_own(&c), 1);
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

    assert_eq!(theorem_own(&before), 2, "lemma は既定で theorem とカウンタを共有する");
    assert_eq!(theorem_own(&after), 1, "theorem クラスの reset_by が共有カウンタを戻し、lemma の番号にも効く");
  }
}
