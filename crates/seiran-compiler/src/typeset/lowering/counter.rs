//! [`CounterValue`]（構造値）から表示文字列を作る純粋関数群
//!
//! カウンタの表示側フィールド（`number_format` / `number_style` / `ref_format` / `ref_number_format`）と
//! `[cross_reference]` の区切りを読むのはこの module だけ。

use std::sync::LazyLock;

use crate::{
  document::TheoremClass,
  semantics::{CounterValue, CounterValues, LabelId, RefGroup},
  style::{CounterName, CounterPlaceholder, RefTemplate, Style},
};

/// 定理の `\ref` 表示に使う固定書式
///
/// カウンタの `ref_format` と違い style に対応するフィールドが無く、cleveref 相当の
/// 「表示名 + 番号」に固定されている。
static THEOREM_REF_FORMAT: LazyLock<RefTemplate> =
  LazyLock::new(|| return RefTemplate::parse("{display_name} {number}"));

/// [`CounterValue`] を、その種別の `number_format` / `number_style` で表示番号にする
///
/// 見出し・図・表・数式・定理の「本体に出る番号」（例: `"1.2"` / `"(1.1)"` の中身）がこれ。
#[must_use]
pub(super) fn format_counter_value(style: &Style, value: &CounterValue) -> String {
  return match *value {
    CounterValue::Counter { name, counters } => expand_counter_template(style, name, &counters),
    CounterValue::Theorem {
      class,
      own,
      counters,
    } => expand_theorem_template(style, class, own, &counters),
  };
}

/// [`CounterValue`] を `\ref` の表示文字列にする（表示番号に `ref_format` を適用したもの）
///
/// `ref_format` の `{number}` には `ref_number_format` を通した番号が入る（定理は番号をそのまま入れる）。
/// 例: `chapter` なら `"Chapter 1"`、`equation` なら `"(1.1)"`、定理なら `"Theorem 1"`。
#[must_use]
pub(super) fn format_ref_display(style: &Style, value: &CounterValue) -> String {
  let number = format_ref_number(style, value);
  return match *value {
    CounterValue::Counter { name, .. } => {
      let def = &style.counters[name];
      def.ref_format.expand(&number, &def.display_name)
    },
    CounterValue::Theorem { class, .. } => THEOREM_REF_FORMAT.expand(&number, &style.theorems[class].display_name),
  };
}

/// 複数参照・範囲参照の表示を文字列の断片の列にする（`Some` は番号 1 個の断片と、そのリンク先のラベル）
///
/// 番号 1 個の断片は [`format_ref_display`] の `{number}` と同じもので、`List` は番号の間に `list_separator` を、
/// `Range` は始点と終点の間に `range_separator` を挟む。その列を外側の `ref_format`（定理は固定書式）の `{number}` へ
/// 入れ、`{display_name}` には複数形の表示名を入れる。種別名と外側の書式は先頭の項目の種類から引く。項目の種類が
/// 揃っていることは `collect_facts` の検証（`MixedRefKinds` / `InvalidRefRange`）が保証する。
#[must_use]
pub(super) fn format_ref_group<'a>(
  style: &Style,
  group: &'a RefGroup,
  value_of: impl Fn(&LabelId) -> &'a CounterValue,
) -> Vec<(String, Option<&'a LabelId>)> {
  let separator = match group {
    RefGroup::List(_) => &style.cross_reference.list_separator,
    RefGroup::Range { .. } => &style.cross_reference.range_separator,
  };
  let numbers = || {
    let mut pieces = Vec::new();
    for (index, label) in group.labels().enumerate() {
      if index > 0 {
        pieces.push((separator.clone(), None));
      }
      pieces.push((format_ref_number(style, value_of(label)), Some(label)));
    }
    return pieces;
  };
  let literal = |text: &str| return (text.to_string(), None);
  let first = group
    .labels()
    .next()
    .expect("RefGroup は参照先を持つ（List は frontend が 2 個以上を保証し、Range は両端を持つ）");
  return match *value_of(first) {
    CounterValue::Counter { name, .. } => {
      let def = &style.counters[name];
      def.ref_format.expand_pieces(&def.display_name_plural, numbers, literal)
    },
    CounterValue::Theorem { class, .. } => {
      THEOREM_REF_FORMAT.expand_pieces(&style.theorems[class].display_name_plural, numbers, literal)
    },
  };
}

/// 参照 1 件ぶんの番号（カウンタは表示番号に `ref_number_format` を適用したもの、定理は表示番号そのまま）
fn format_ref_number(style: &Style, value: &CounterValue) -> String {
  let number = format_counter_value(style, value);
  return match *value {
    CounterValue::Counter { name, .. } => style.counters[name].ref_number_format.expand(&number),
    CounterValue::Theorem { .. } => number,
  };
}

/// カウンタの `number_format`（`"{chapter}.{n}"` 等）を、採番時点の全カウンタの値で展開する
///
/// `{n}` は自身、`{<counter_name>}` は同名のカウンタを指し、いずれも**参照先カウンタ自身の**
/// `number_style` で描画する（`{part}` は既定でローマ数字）。
fn expand_counter_template(style: &Style, name: CounterName, counters: &CounterValues) -> String {
  return style.counters[name].number_format.expand(|placeholder| {
    let target = match placeholder {
      CounterPlaceholder::Own => name,
      CounterPlaceholder::Counter(target) => target,
    };
    return render_named(style, counters, target);
  });
}

/// 定理の `number_format`（`"{n}"` / `"{chapter}.{n}"` 等）を、採番時点の全カウンタの値で展開する
///
/// `{n}` は定理の共有カウンタの値 `own` で、カウンタと違い `number_style` を持たないため素の 10 進数で
/// 描画する。
fn expand_theorem_template(style: &Style, class: TheoremClass, own: u32, counters: &CounterValues) -> String {
  return style.theorems[class].number_format.expand(|placeholder| {
    return match placeholder {
      CounterPlaceholder::Own => own.to_string(),
      CounterPlaceholder::Counter(target) => render_named(style, counters, target),
    };
  });
}

/// `target` カウンタの値を、`target` 自身の `number_style` で描画する
fn render_named(style: &Style, counters: &CounterValues, target: CounterName) -> String {
  return style.counters[target].number_style.render(counters[target]);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    semantics::{LabelId, RefGroup},
    style::{CounterTemplate, NumberStyle, RefNumberTemplate, RefTemplate, TheoremReset},
  };

  /// `(カウンタ名, 値)` の列を全カウンタの値にする（列に無いカウンタは 0）
  fn snapshot(values: &[(CounterName, u32)]) -> CounterValues {
    let mut counters = CounterValues::default();
    for &(name, value) in values {
      counters[name] = value;
    }
    return counters;
  }

  /// カウンタ `name` で採番した構造値を組み立てる（自身の値も `values` に含める）
  fn counter_value(name: CounterName, values: &[(CounterName, u32)]) -> CounterValue {
    return CounterValue::Counter {
      name,
      counters: snapshot(values),
    };
  }

  /// 定理クラス `class` で採番した構造値を組み立てる
  fn theorem_value(class: TheoremClass, values: &[(CounterName, u32)], own: u32) -> CounterValue {
    return CounterValue::Theorem {
      class,
      own,
      counters: snapshot(values),
    };
  }

  #[test]
  fn part_uses_roman_upper_number_style() {
    let style = Style::default();

    let text = format_counter_value(&style, &counter_value(CounterName::Part, &[(CounterName::Part, 2)]));

    assert_eq!(text, "II");
  }

  #[test]
  fn section_embeds_chapter_value() {
    let style = Style::default();
    let value = counter_value(CounterName::Section, &[(CounterName::Chapter, 1), (CounterName::Section, 2)]);

    let text = format_counter_value(&style, &value);

    assert_eq!(text, "1.2", "既定の section は number_format = \"{{chapter}}.{{n}}\"");
  }

  #[test]
  fn own_name_placeholder_resolves_to_own_value() {
    let mut style = Style::default();
    style.counters.section.number_format = CounterTemplate::parse("{section}");
    let value = counter_value(CounterName::Section, &[(CounterName::Chapter, 1), (CounterName::Section, 4)]);

    let text = format_counter_value(&style, &value);

    assert_eq!(text, "4", "自身名のプレースホルダは自身の値を指す");
  }

  #[test]
  fn cross_counter_placeholder_uses_target_number_style() {
    let mut style = Style::default();
    style.counters.chapter.number_format = CounterTemplate::parse("{part}-{n}");
    style.counters.chapter.number_style = NumberStyle::Arabic;
    let value = counter_value(CounterName::Chapter, &[(CounterName::Part, 2), (CounterName::Chapter, 1)]);

    let text = format_counter_value(&style, &value);

    assert_eq!(text, "II-1", "part は part 自身の number_style で描画される");
  }

  #[test]
  fn section_without_chapter_drops_the_separator() {
    let style = Style::default();
    let value = counter_value(CounterName::Section, &[(CounterName::Section, 1)]);

    let number = format_counter_value(&style, &value);
    let reference = format_ref_display(&style, &value);

    assert_eq!(number, "1", "未登場の chapter は直後の区切りごと省く");
    assert_eq!(reference, "Section 1");
  }

  #[test]
  fn theorem_without_reset_by_counter_drops_the_separator() {
    let mut style = Style::default();
    style.theorems.theorem.number_format = CounterTemplate::parse("{chapter}.{n}");

    let text = format_counter_value(&style, &theorem_value(TheoremClass::Theorem, &[], 2));

    assert_eq!(text, "2");
  }

  #[test]
  fn chapter_ref_display_applies_ref_format() {
    let style = Style::default();

    let text = format_ref_display(&style, &counter_value(CounterName::Chapter, &[(CounterName::Chapter, 1)]));

    assert_eq!(text, "Chapter 1");
  }

  #[test]
  fn equation_ref_display_is_unchanged() {
    let style = Style::default();

    let text = format_ref_display(&style, &counter_value(CounterName::Equation, &[(CounterName::Equation, 1)]));

    assert_eq!(text, "(1)");
  }

  #[test]
  fn ref_display_applies_ref_number_format_inside_ref_format() {
    let mut style = Style::default();
    style.counters.equation.ref_format = RefTemplate::parse("Eq. {number}");
    style.counters.equation.ref_number_format = RefNumberTemplate::parse("[{number}]");

    let text = format_ref_display(&style, &counter_value(CounterName::Equation, &[(CounterName::Equation, 1)]));

    assert_eq!(text, "Eq. [1]");
  }

  #[test]
  fn theorem_number_embeds_reset_by_counter() {
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = CounterTemplate::parse("{section}.{n}");
    let value = theorem_value(TheoremClass::Theorem, &[(CounterName::Section, 3)], 1);

    let text = format_counter_value(&style, &value);

    assert_eq!(text, "3.1");
  }

  #[test]
  fn theorem_number_embeds_counters_other_than_reset_by() {
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = CounterTemplate::parse("{chapter}.{section}.{n}");
    let value = theorem_value(TheoremClass::Theorem, &[(CounterName::Chapter, 2), (CounterName::Section, 3)], 1);

    let text = format_counter_value(&style, &value);

    assert_eq!(text, "2.3.1");
  }

  #[test]
  fn theorem_ref_display_uses_display_name_and_number() {
    let style = Style::default();

    let text = format_ref_display(&style, &theorem_value(TheoremClass::Lemma, &[], 4));

    assert_eq!(text, "Lemma 4");
  }

  #[test]
  fn counter_outside_reset_relation_is_displayed() {
    let mut style = Style::default();
    style.counters.figure.number_format = CounterTemplate::parse("{section}.{n}");
    let value = counter_value(
      CounterName::Figure,
      &[
        (CounterName::Chapter, 1),
        (CounterName::Section, 3),
        (CounterName::Figure, 5),
      ],
    );

    let text = format_counter_value(&style, &value);

    assert_eq!(text, "3.5", "section は図をリセットしないが、採番時点の値を表示できる");
  }

  #[test]
  fn ref_display_of_counter_outside_reset_relation_uses_its_number() {
    let mut style = Style::default();
    style.counters.figure.number_format = CounterTemplate::parse("{section}.{n}");
    let value = counter_value(CounterName::Figure, &[(CounterName::Section, 1), (CounterName::Figure, 1)]);

    let text = format_ref_display(&style, &value);

    assert_eq!(text, "Figure 1.1");
  }

  /// `entries` からラベルの値を引く関数を作る（無いラベルはテストの組み立て誤り）
  fn values_of<'a>(entries: &'a [(LabelId, CounterValue)]) -> impl Fn(&LabelId) -> &'a CounterValue {
    return move |label| {
      return entries
        .iter()
        .find_map(|(key, value)| return (key == label).then_some(value))
        .expect("テストはすべてのラベルの値を渡すはず");
    };
  }

  /// 断片列を文字列として連結する
  fn joined(pieces: &[(String, Option<&LabelId>)]) -> String {
    return pieces.iter().map(|(text, _)| return text.as_str()).collect();
  }

  #[test]
  fn ref_list_links_each_number() {
    let style = Style::default();
    let (f1, f3) = (LabelId::new("f1"), LabelId::new("f3"));
    let entries = [
      (f1.clone(), counter_value(CounterName::Figure, &[(CounterName::Figure, 1)])),
      (f3.clone(), counter_value(CounterName::Figure, &[(CounterName::Figure, 3)])),
    ];
    let group = RefGroup::List(vec![f1.clone(), f3.clone()]);

    let pieces = format_ref_group(&style, &group, values_of(&entries));

    assert_eq!(
      pieces,
      vec![
        ("Figures ".to_string(), None),
        ("1".to_string(), Some(&f1)),
        (", ".to_string(), None),
        ("3".to_string(), Some(&f3)),
      ]
    );
  }

  #[test]
  fn ref_range_wraps_each_end_with_ref_number_format() {
    let style = Style::default();
    let (e1, e3) = (LabelId::new("e1"), LabelId::new("e3"));
    let entries = [
      (e1.clone(), counter_value(CounterName::Equation, &[(CounterName::Equation, 1)])),
      (e3.clone(), counter_value(CounterName::Equation, &[(CounterName::Equation, 3)])),
    ];
    let group = RefGroup::Range {
      first: e1.clone(),
      last: e3.clone(),
    };

    let pieces = format_ref_group(&style, &group, values_of(&entries));

    assert_eq!(
      pieces,
      vec![
        ("(1)".to_string(), Some(&e1)),
        ("\u{2013}".to_string(), None),
        ("(3)".to_string(), Some(&e3)),
      ]
    );
  }

  #[test]
  fn theorem_ref_list_uses_plural_display_name() {
    let style = Style::default();
    let (t1, t2) = (LabelId::new("t1"), LabelId::new("t2"));
    let entries = [
      (t1.clone(), theorem_value(TheoremClass::Theorem, &[], 1)),
      (t2.clone(), theorem_value(TheoremClass::Theorem, &[], 2)),
    ];
    let group = RefGroup::List(vec![t1.clone(), t2.clone()]);

    let pieces = format_ref_group(&style, &group, values_of(&entries));

    assert_eq!(joined(&pieces), "Theorems 1, 2");
    assert_eq!(pieces[1], ("1".to_string(), Some(&t1)));
    assert_eq!(pieces[3], ("2".to_string(), Some(&t2)));
  }

  #[test]
  fn same_refs_change_notation_with_style_only() {
    let mut style = Style::default();
    style.counters.figure.display_name = "図".to_string();
    style.counters.figure.display_name_plural = "図".to_string();
    style.counters.figure.ref_format = RefTemplate::parse("{display_name}{number}");
    style.counters.equation.display_name = "式".to_string();
    style.counters.equation.display_name_plural = "式".to_string();
    style.counters.equation.ref_format = RefTemplate::parse("{display_name}{number}");
    style.cross_reference.list_separator = "、".to_string();
    style.cross_reference.range_separator = "〜".to_string();
    let (f1, f3, e1, e3) = (LabelId::new("f1"), LabelId::new("f3"), LabelId::new("e1"), LabelId::new("e3"));
    let entries = [
      (f1.clone(), counter_value(CounterName::Figure, &[(CounterName::Figure, 1)])),
      (f3.clone(), counter_value(CounterName::Figure, &[(CounterName::Figure, 3)])),
      (e1.clone(), counter_value(CounterName::Equation, &[(CounterName::Equation, 1)])),
      (e3.clone(), counter_value(CounterName::Equation, &[(CounterName::Equation, 3)])),
    ];
    let figures = RefGroup::List(vec![f1, f3]);
    let equations = RefGroup::Range {
      first: e1,
      last: e3,
    };

    let figure_pieces = format_ref_group(&style, &figures, values_of(&entries));
    let equation_pieces = format_ref_group(&style, &equations, values_of(&entries));

    assert_eq!(joined(&figure_pieces), "図1、3");
    assert_eq!(joined(&equation_pieces), "式(1)〜(3)");
  }
}
