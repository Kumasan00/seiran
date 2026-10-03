//! 意味解析（`crate::semantics`）が必要とする設定だけを抜き出した投影 [`SemanticPolicy`]。
//!
//! `style.toml` の表示側フィールド（`number_format` / `ref_format` / `display_name` /
//! `number_style`）はここに写さない。意味解析が表示設定を読めないことを型として保証する境界。

use std::collections::HashMap;

use strum::VariantArray;

use crate::{
  document::TheoremClass,
  style::{CounterName, Style},
};

/// 1 カウンタぶんの値側設定
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CounterPolicy {
  /// このカウンタが増えたときに 0 へ戻す下位カウンタ
  pub resets: Vec<CounterName>,
  /// このカウンタが増えたときに 0 へ戻す定理の共有カウンタ名（`reset_by` がこのカウンタを指す定理クラスの
  /// `counter` を `TheoremClass::VARIANTS` の順に並べる。複数クラスが同じ共有カウンタを持つと重複する）
  pub theorem_resets: Vec<String>,
}

/// 1 定理クラスぶんの値側設定
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TheoremPolicy {
  /// 共有カウンタ名（複数クラスが 1 つのカウンタを共有しうる）
  pub counter: String,
  /// リセット元の見出しカウンタ。定理カウンタの唯一の祖先でもある（`None` はリセットしない）
  pub reset_by: Option<CounterName>,
  /// 無採番クラス（`proof`）かどうか
  pub unnumbered: bool,
}

/// 意味解析が読む設定だけを持つ投影
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SemanticPolicy {
  /// カウンタ名 → 値側設定（固定 9 種すべてを `from_style` が埋める）
  counters: HashMap<CounterName, CounterPolicy>,
  /// 定理クラス → 値側設定（全クラスを `from_style` が埋める）
  theorems: HashMap<TheoremClass, TheoremPolicy>,
}

impl SemanticPolicy {
  /// `Style` から値側設定だけを写し取る
  ///
  /// `reset_by` の `TheoremReset` → [`CounterName`] の写像はここで適用し、意味解析の残りは `CounterName` だけを読む
  #[must_use]
  pub(crate) fn from_style(style: &Style) -> Self {
    let mut counters = HashMap::new();
    for &name in CounterName::VARIANTS {
      let theorem_resets = TheoremClass::VARIANTS
        .iter()
        .map(|&class| return &style.theorems[class])
        .filter(|def| return def.reset_by.counter_name() == Some(name))
        .map(|def| return def.counter.clone())
        .collect();
      counters.insert(
        name,
        CounterPolicy {
          resets: style.counters[name].resets.clone(),
          theorem_resets,
        },
      );
    }
    let mut theorems = HashMap::new();
    for &class in TheoremClass::VARIANTS {
      let def = &style.theorems[class];
      theorems.insert(
        class,
        TheoremPolicy {
          counter: def.counter.clone(),
          reset_by: def.reset_by.counter_name(),
          unnumbered: def.unnumbered,
        },
      );
    }
    return SemanticPolicy { counters, theorems };
  }

  /// カウンタの値側設定を引く
  #[must_use]
  pub(super) fn counter(&self, name: CounterName) -> &CounterPolicy {
    let Some(policy) = self.counters.get(&name) else {
      unreachable!("from_style が CounterName::VARIANTS をすべて埋めている: {name:?}")
    };
    return policy;
  }

  /// 定理クラスの値側設定を引く
  #[must_use]
  pub(super) fn theorem(&self, class: TheoremClass) -> &TheoremPolicy {
    let Some(policy) = self.theorems.get(&class) else {
      unreachable!("from_style が全定理クラスを埋めている: {class:?}")
    };
    return policy;
  }
}

#[cfg(test)]
mod tests {
  use super::SemanticPolicy;
  use crate::{
    document::TheoremClass,
    style::{CounterName, CounterTemplate, NumberStyle, RefTemplate, Style, TheoremReset},
  };

  #[test]
  fn document_policy_ignores_display_only_style_fields() {
    let base = Style::default();
    let mut display_variant = Style::default();
    display_variant.counters.chapter.number_format = CounterTemplate::parse("第{n}章");
    display_variant.counters.chapter.ref_format = RefTemplate::parse("{display_name}（{number}）");
    display_variant.counters.chapter.display_name = "章".to_string();
    display_variant.counters.chapter.number_style = NumberStyle::RomanUpper;

    let base_policy = SemanticPolicy::from_style(&base);
    let variant_policy = SemanticPolicy::from_style(&display_variant);

    assert_eq!(base_policy, variant_policy, "表示専用フィールドは SemanticPolicy に写らないはず");
  }

  #[test]
  fn document_policy_reflects_value_affecting_style_fields() {
    let base = Style::default();
    let mut reset_variant = Style::default();
    reset_variant.counters.chapter.resets = vec![];

    let base_policy = SemanticPolicy::from_style(&base);
    let variant_policy = SemanticPolicy::from_style(&reset_variant);

    assert_ne!(base_policy, variant_policy, "resets は値側フィールドなので SemanticPolicy に写るはず");
    assert_eq!(variant_policy.counter(CounterName::Chapter).resets, []);
  }

  #[test]
  fn theorem_reset_by_projects_onto_its_heading_counter_only() {
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;

    let policy = SemanticPolicy::from_style(&style);

    assert_eq!(policy.theorem(TheoremClass::Theorem).reset_by, Some(CounterName::Section));
    assert_eq!(
      policy.counter(CounterName::Section).theorem_resets,
      ["theorem"],
      "節が進むと theorem クラスの共有カウンタが 0 に戻る"
    );
    assert!(
      policy.counter(CounterName::Chapter).theorem_resets.is_empty(),
      "reset_by = section の定理は章が進んでも直接は戻らない"
    );
  }
}
