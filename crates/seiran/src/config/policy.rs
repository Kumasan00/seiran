//! 意味解析（`crate::resolve`）が必要とする設定だけを抜き出した投影 [`DocumentPolicy`]。
//!
//! `style.toml` の表示側フィールド（`number_format` / `ref_format` / `display_name` /
//! `number_style`）はここに写さない。意味解析が表示設定を読めないことを、規約や property test
//! ではなく型として保証するための境界（issue #324）。写すのは採番の値に影響する
//! `resets` / `counter` / `reset_by` / `unnumbered` と、見出しレベル → カウンタ名の写像だけ。

use std::collections::HashMap;

use crate::{
  config::{CounterName, Style, TheoremReset},
  model::{HeadingLevel, TheoremClass},
};

/// 1 カウンタぶんの値側設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterPolicy {
  /// このカウンタが増えたときに 0 へ戻す下位カウンタ
  pub resets: Vec<CounterName>,
}

/// 1 定理クラスぶんの値側設定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheoremPolicy {
  /// 共有カウンタ名（複数クラスが 1 つのカウンタを共有しうる）
  pub counter: String,
  /// どの見出しレベルでリセットするか
  pub reset_by: TheoremReset,
  /// 無採番クラス（`proof`）かどうか
  pub unnumbered: bool,
}

/// 意味解析が読む設定だけを持つ投影
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentPolicy {
  /// カウンタ名 → 値側設定（固定 9 種すべてを `from_style` が埋める）
  counters: HashMap<CounterName, CounterPolicy>,
  /// 定理クラス → 値側設定（全クラスを `from_style` が埋める）
  theorems: HashMap<TheoremClass, TheoremPolicy>,
}

impl DocumentPolicy {
  /// `Style` から値側設定だけを写し取る
  #[must_use]
  pub fn from_style(style: &Style) -> Self {
    let mut counters = HashMap::new();
    for name in CounterName::ALL {
      counters.insert(
        name,
        CounterPolicy {
          resets: style.counters.get(name).resets.clone(),
        },
      );
    }
    let mut theorems = HashMap::new();
    for (class, def) in style.theorems.iter_with_class() {
      theorems.insert(
        class,
        TheoremPolicy {
          counter: def.counter.clone(),
          reset_by: def.reset_by,
          unnumbered: def.unnumbered,
        },
      );
    }
    return DocumentPolicy { counters, theorems };
  }

  /// カウンタの値側設定を引く
  #[must_use]
  pub fn counter(&self, name: CounterName) -> &CounterPolicy {
    let Some(policy) = self.counters.get(&name) else {
      unreachable!("from_style が CounterName::ALL をすべて埋めている: {name:?}")
    };
    return policy;
  }

  /// 定理クラスの値側設定を引く
  #[must_use]
  pub fn theorem(&self, class: TheoremClass) -> &TheoremPolicy {
    let Some(policy) = self.theorems.get(&class) else {
      unreachable!("from_style が全定理クラスを埋めている: {class:?}")
    };
    return policy;
  }

  /// 指定した見出しレベルでリセットされる定理カウンタ名を列挙する
  pub fn theorems_reset_by(&self, level: TheoremReset) -> impl Iterator<Item = &str> {
    return self
      .theorems
      .values()
      .filter(move |policy| return policy.reset_by == level)
      .map(|policy| return policy.counter.as_str());
  }

  /// 見出しレベルに対応するカウンタ名を返す
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::DocumentPolicy;
  use crate::{
    config::{CounterName, NumberStyle, Style},
    model::HeadingLevel,
  };

  #[test]
  fn document_policy_ignores_display_only_style_fields() {
    // Arrange — 表示専用フィールドだけが異なる 2 つの Style
    let base = Style::default();
    let mut display_variant = Style::default();
    display_variant.counters.chapter.number_format = "第{n}章".to_string();
    display_variant.counters.chapter.ref_format = "{display_name}（{number}）".to_string();
    display_variant.counters.chapter.display_name = "章".to_string();
    display_variant.counters.chapter.number_style = NumberStyle::RomanUpper;

    // Act
    let base_policy = DocumentPolicy::from_style(&base);
    let variant_policy = DocumentPolicy::from_style(&display_variant);

    // Assert
    assert_eq!(base_policy, variant_policy, "表示専用フィールドは DocumentPolicy に写らないはず");
  }

  #[test]
  fn document_policy_reflects_value_affecting_style_fields() {
    // Arrange — resets（値側フィールド）だけが異なる Style
    let base = Style::default();
    let mut reset_variant = Style::default();
    reset_variant.counters.chapter.resets = vec![];

    // Act
    let base_policy = DocumentPolicy::from_style(&base);
    let variant_policy = DocumentPolicy::from_style(&reset_variant);

    // Assert
    assert_ne!(base_policy, variant_policy, "resets は値側フィールドなので DocumentPolicy に写るはず");
    assert!(variant_policy.counter(CounterName::Chapter).resets.is_empty());
  }

  #[test]
  fn counter_name_for_heading_maps_each_level() {
    assert_eq!(DocumentPolicy::counter_name_for_heading(HeadingLevel::Part), CounterName::Part);
    assert_eq!(DocumentPolicy::counter_name_for_heading(HeadingLevel::Chapter), CounterName::Chapter);
    assert_eq!(DocumentPolicy::counter_name_for_heading(HeadingLevel::Subparagraph), CounterName::Subparagraph);
  }
}
