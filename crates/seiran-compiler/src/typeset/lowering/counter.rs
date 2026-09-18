//! [`CounterValue`]（構造値）から表示文字列を作る純粋関数群
//!
//! issue #282 の「値と表示の分離」の表示側。`semantics` module は `resets` / `reset_by`
//! （値に影響する style フィールド）だけを読んでカウンタの構造値を確定させ、
//! `number_format` / `number_style` / `ref_format`（表示側フィールド）はこのモジュールだけが読む。
//!
//! 値そのものを作る側はここに無い — ラベル登録・カウンタ値算出（旧 `CounterRegistry`）は #282 で
//! `semantics` へ、脚注の出現 index 発番は `LoweringState` へ、確定ページ列からのページ単位脚注
//! 表示番号の割り当ては `typeset::pagination::footnote_numbering` へ移してある。

use std::sync::LazyLock;

use crate::{
  document::TheoremClass,
  semantics::{CounterKind, CounterValue},
  style::{CounterName, CounterPlaceholder, ReferenceTemplate, Style},
};

/// 定理の `\ref` 表示に使う固定書式
///
/// カウンタの `ref_format` と違い style に対応するフィールドが無く、cleveref 相当の
/// 「表示名 + 番号」に固定されている（issue #282 以前の `CounterRegistry` から引き継いだ挙動）。
/// style 由来のテンプレートと同じく、解析は 1 回だけ行う。
static THEOREM_REF_FORMAT: LazyLock<ReferenceTemplate> =
  LazyLock::new(|| return ReferenceTemplate::parse("{display_name} {number}"));

/// [`CounterValue`] を、その種別の `number_format` / `number_style` で表示番号にする
///
/// 見出し・図・表・数式・定理の「本体に出る番号」（例: `"1.2"` / `"(1.1)"` の中身）がこれ。
/// `\ref` の表示文字列が欲しい場合は [`format_ref_display`] を使う。
#[must_use]
pub(crate) fn format_counter_value(style: &Style, value: &CounterValue) -> String {
  return match value.kind {
    CounterKind::Counter(name) => expand_counter_template(style, name, value),
    CounterKind::Theorem(class) => expand_theorem_template(style, class, value),
  };
}

/// [`CounterValue`] を `\ref` の表示文字列にする（表示番号に `ref_format` を適用したもの）
///
/// 例: `chapter` なら `"Chapter 1"`、`equation` なら `"(1.1)"`、定理なら `"Theorem 1"`。
#[must_use]
pub(crate) fn format_ref_display(style: &Style, value: &CounterValue) -> String {
  let number = format_counter_value(style, value);
  return match value.kind {
    CounterKind::Counter(name) => {
      let def = &style.counters[name];
      def.ref_format.expand(&number, &def.display_name)
    },
    CounterKind::Theorem(class) => THEOREM_REF_FORMAT.expand(&number, &style.theorems[class].display_name),
  };
}

/// カウンタの `number_format`（`"{chapter}.{n}"` 等）を、構造値を使って展開する
///
/// `{n}` は自身、`{<counter_name>}` は同名のカウンタ（自身または祖先）を指し、いずれも
/// **参照先カウンタ自身の** `number_style` で描画する（`{part}` は既定でローマ数字）。
fn expand_counter_template(style: &Style, name: CounterName, value: &CounterValue) -> String {
  return style.counters[name].number_format.expand(|placeholder| {
    let target = match placeholder {
      CounterPlaceholder::Own => name,
      CounterPlaceholder::Counter(target) => target,
    };
    return render_named(style, value, target);
  });
}

/// 定理の `number_format`（`"{n}"` / `"{chapter}.{n}"` 等）を、構造値を使って展開する
///
/// `{n}` は定理カウンタ自身の値で、カウンタと違い `number_style` を持たないため素の 10 進数で
/// 描画する。`{<counter_name>}` は構造値の祖先 — `reset_by` が指す見出しカウンタ 1 段だけ —
/// を名前で引く。
fn expand_theorem_template(style: &Style, class: TheoremClass, value: &CounterValue) -> String {
  return style.theorems[class].number_format.expand(|placeholder| {
    return match placeholder {
      CounterPlaceholder::Own => value.own.to_string(),
      CounterPlaceholder::Counter(target) => render_named(style, value, target),
    };
  });
}

/// 構造値に載っている `target` カウンタの値を、`target` 自身の `number_style` で描画する
///
/// 値に載っていないカウンタ — 例えば `number_format = "{section}.{n}"` の図（既定では
/// `section` は図の祖先ではない）— は復元できないため空文字列にする。issue #282 以前は
/// 採番時点の現在値を読めたため、この点だけは表示が退行している。
fn render_named(style: &Style, value: &CounterValue, target: CounterName) -> String {
  let Some(number) = value.value_of(target) else {
    return String::new();
  };
  return style.counters[target].number_style.render(number);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    semantics::CounterPart,
    style::{CounterTemplate, NumberStyle, TheoremReset},
  };

  /// `CounterKind::Counter` の構造値を組み立てるテストヘルパ
  fn counter_value(name: CounterName, ancestors: &[(CounterName, u32)], own: u32) -> CounterValue {
    return CounterValue {
      kind: CounterKind::Counter(name),
      ancestors: parts(ancestors),
      own,
    };
  }

  /// `CounterKind::Theorem` の構造値を組み立てるテストヘルパ
  fn theorem_value(class: TheoremClass, ancestors: &[(CounterName, u32)], own: u32) -> CounterValue {
    return CounterValue {
      kind: CounterKind::Theorem(class),
      ancestors: parts(ancestors),
      own,
    };
  }

  /// `(カウンタ名, 値)` の列を祖先チェーンに変換する
  fn parts(ancestors: &[(CounterName, u32)]) -> Vec<CounterPart> {
    return ancestors
      .iter()
      .map(|(name, value)| {
        return CounterPart {
          name: *name,
          value: *value,
        };
      })
      .collect();
  }

  #[test]
  fn chapter_uses_arabic_number_only() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Chapter, &[(CounterName::Part, 0)], 1));

    // Assert
    assert_eq!(text, "1");
  }

  #[test]
  fn part_uses_roman_upper_number_style() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Part, &[], 2));

    // Assert
    assert_eq!(text, "II");
  }

  #[test]
  fn section_embeds_ancestor_chapter_value() {
    // Arrange
    let style = Style::default();
    let value = counter_value(CounterName::Section, &[(CounterName::Part, 0), (CounterName::Chapter, 1)], 2);

    // Act
    let text = format_counter_value(&style, &value);

    // Assert
    assert_eq!(text, "1.2", "既定の section は number_format = \"{{chapter}}.{{n}}\"");
  }

  #[test]
  fn subsection_embeds_two_ancestor_values() {
    // Arrange
    let style = Style::default();
    let ancestors = [
      (CounterName::Part, 0),
      (CounterName::Chapter, 1),
      (CounterName::Section, 2),
    ];
    let value = counter_value(CounterName::Subsection, &ancestors, 3);

    // Act
    let text = format_counter_value(&style, &value);

    // Assert
    assert_eq!(text, "1.2.3");
  }

  #[test]
  fn own_name_placeholder_resolves_to_own_value() {
    // Arrange — 自身のカウンタ名で自身を参照する書式（`{n}` と同じ値を指す）
    let mut style = Style::default();
    style.counters.section.number_format = CounterTemplate::parse("{section}");
    let value = counter_value(CounterName::Section, &[(CounterName::Part, 0), (CounterName::Chapter, 1)], 4);

    // Act
    let text = format_counter_value(&style, &value);

    // Assert
    assert_eq!(text, "4", "自身名のプレースホルダは own を指す");
  }

  #[test]
  fn literal_decoration_in_number_format_is_kept() {
    // Arrange
    let mut style = Style::default();
    style.counters.chapter.number_format = CounterTemplate::parse("第{n}章");

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Chapter, &[(CounterName::Part, 0)], 3));

    // Assert
    assert_eq!(text, "第3章");
  }

  #[test]
  fn cross_counter_reference_uses_target_number_style() {
    // Arrange — part は RomanUpper、chapter は Arabic
    let mut style = Style::default();
    style.counters.chapter.number_format = CounterTemplate::parse("{part}-{n}");
    style.counters.chapter.number_style = NumberStyle::Arabic;

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Chapter, &[(CounterName::Part, 2)], 1));

    // Assert
    assert_eq!(text, "II-1", "祖先 part は part 自身の number_style で描画される");
  }

  #[test]
  fn chapter_ref_display_applies_ref_format() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_ref_display(&style, &counter_value(CounterName::Chapter, &[(CounterName::Part, 0)], 1));

    // Assert
    assert_eq!(text, "Chapter 1");
  }

  #[test]
  fn equation_ref_display_uses_parenthesized_ref_format() {
    // Arrange
    let style = Style::default();
    let value = counter_value(CounterName::Equation, &[(CounterName::Part, 0), (CounterName::Chapter, 1)], 1);

    // Act
    let text = format_ref_display(&style, &value);

    // Assert
    assert_eq!(text, "(1.1)");
  }

  #[test]
  fn theorem_number_uses_plain_own_value() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &theorem_value(TheoremClass::Theorem, &[], 2));

    // Assert
    assert_eq!(text, "2");
  }

  #[test]
  fn theorem_number_embeds_reset_by_counter() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = CounterTemplate::parse("{section}.{n}");
    let value = theorem_value(TheoremClass::Theorem, &[(CounterName::Section, 3)], 1);

    // Act
    let text = format_counter_value(&style, &value);

    // Assert
    assert_eq!(text, "3.1");
  }

  #[test]
  fn theorem_reference_to_counter_off_the_value_is_empty() {
    // Arrange — reset_by は section なので、chapter の値は構造値に載っていない
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = CounterTemplate::parse("{chapter}.{n}");
    let value = theorem_value(TheoremClass::Theorem, &[(CounterName::Section, 3)], 1);

    // Act
    let text = format_counter_value(&style, &value);

    // Assert
    assert_eq!(text, ".1", "祖先に無いカウンタ参照は空文字列になる（既知の制限）");
  }

  #[test]
  fn theorem_ref_display_uses_display_name_and_number() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_ref_display(&style, &theorem_value(TheoremClass::Lemma, &[], 4));

    // Assert
    assert_eq!(text, "Lemma 4");
  }

  #[test]
  fn counter_not_on_ancestor_chain_renders_empty_placeholder() {
    // Arrange — 図の既定の祖先は chapter なので、section の値は構造値に含まれない
    let mut style = Style::default();
    style.counters.figure.number_format = CounterTemplate::parse("{section}.{n}");
    let value = counter_value(CounterName::Figure, &[(CounterName::Part, 0), (CounterName::Chapter, 1)], 5);

    // Act
    let text = format_counter_value(&style, &value);

    // Assert
    assert_eq!(text, ".5", "復元できない他カウンタ参照は空文字列になる（既知の制限）");
  }
}
