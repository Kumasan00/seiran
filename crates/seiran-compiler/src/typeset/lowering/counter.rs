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
  style::{CounterName, CounterPlaceholder, Counters, ReferenceTemplate, Style, TheoremReset},
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
    CounterKind::Counter(name) => expand_counter_template(style, name, &value.parts),
    CounterKind::Theorem(class) => expand_theorem_template(style, class, &value.parts),
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

/// カウンタの `number_format`（`"{chapter}.{n}"` 等）を、構造値 `parts` を使って展開する
///
/// `{n}` は自身、`{<counter_name>}` は祖先チェーン上の同名カウンタを指し、いずれも
/// **参照先カウンタ自身の** `number_style` で描画する（`{part}` は既定でローマ数字）。
fn expand_counter_template(style: &Style, name: CounterName, parts: &[u32]) -> String {
  let chain = counter_chain(&style.counters, name);
  return style.counters[name].number_format.expand(|placeholder| {
    let target = match placeholder {
      CounterPlaceholder::Own => name,
      CounterPlaceholder::Counter(target) => target,
    };
    return render_from_chain(style, &chain, parts, target);
  });
}

/// 定理の `number_format`（`"{n}"` / `"{chapter}.{n}"` 等）を、構造値 `parts` を使って展開する
///
/// `{n}` は定理カウンタ自身の値で、カウンタと違い `number_style` を持たないため素の 10 進数で
/// 描画する。`{<counter_name>}` は `reset_by` が指す見出しカウンタのみ解決できる
/// （`semantics` が構造値に載せる祖先はその 1 段だけ — [`counter_chain`] の制限と同じ理由）。
fn expand_theorem_template(style: &Style, class: TheoremClass, parts: &[u32]) -> String {
  let def = &style.theorems[class];
  let own = parts.last().copied().unwrap_or(0);
  let reset_counter = theorem_reset_counter_name(def.reset_by);
  return def.number_format.expand(|placeholder| {
    let CounterPlaceholder::Counter(target) = placeholder else {
      return own.to_string();
    };
    if reset_counter != Some(target) {
      return String::new();
    }
    // 祖先（reset_by の見出しカウンタ）は parts の先頭に 1 つだけ載る
    let Some(ancestor) = parts.first().filter(|_| return parts.len() >= 2) else {
      return String::new();
    };
    return style.counters[target].number_style.render(*ancestor);
  });
}

/// 祖先チェーン上の `target` カウンタの値を、`target` 自身の `number_style` で描画する
///
/// `chain` と `parts` は同じ長さ・同じ順序で対応する。チェーンに無いカウンタは
/// 構造値から値を復元できないため空文字列にする（[`counter_chain`] の doc コメント参照）。
fn render_from_chain(style: &Style, chain: &[CounterName], parts: &[u32], target: CounterName) -> String {
  let Some(index) = chain.iter().position(|candidate| return *candidate == target) else {
    return String::new();
  };
  let Some(value) = parts.get(index) else {
    return String::new();
  };
  return style.counters[target].number_style.render(*value);
}

/// `name` の祖先カウンタ列に自身を足した列を返す（`CounterValue::parts` と 1:1 に対応する）
///
/// 親の求め方は `semantics` 側の `CounterRegistry::ancestor_values` と同一に保つ必要がある
/// （「自分を `resets` に含み、`CounterName::ALL` の宣言順で自身より手前にあるカウンタのうち
/// 最も近いもの」）。`parts` は祖先を辿った順（末尾が自身）で積まれるため、この列と添字が揃う。
///
/// この対応が付かないカウンタ — 例えば `number_format = "{section}.{n}"` の図（既定では
/// `section` は図の祖先ではない）— の値は構造値に含まれておらず復元できない。issue #282 以前は
/// 採番時点の現在値を読めたため、この点だけは表示が退行する（空文字列になる）。
fn counter_chain(counters: &Counters, name: CounterName) -> Vec<CounterName> {
  let mut chain = ancestor_chain(counters, name);
  chain.push(name);
  return chain;
}

/// `name` の祖先カウンタを、最も遠い祖先から順に集める（末尾が直近の親）
fn ancestor_chain(counters: &Counters, name: CounterName) -> Vec<CounterName> {
  let own_index = CounterName::ALL
    .iter()
    .position(|candidate| return *candidate == name)
    .expect("CounterName::ALL は全 9 バリアントを含む");
  let parent = CounterName::ALL[..own_index]
    .iter()
    .rev()
    .find(|candidate| return counters[**candidate].resets.contains(&name))
    .copied();
  let Some(parent) = parent else {
    return Vec::new();
  };
  let mut chain = ancestor_chain(counters, parent);
  chain.push(parent);
  return chain;
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
  use super::*;
  use crate::style::{CounterTemplate, NumberStyle};

  /// `CounterKind::Counter` の構造値を組み立てるテストヘルパ
  fn counter_value(name: CounterName, parts: &[u32]) -> CounterValue {
    return CounterValue {
      kind: CounterKind::Counter(name),
      parts: parts.to_vec(),
    };
  }

  /// `CounterKind::Theorem` の構造値を組み立てるテストヘルパ
  fn theorem_value(class: TheoremClass, parts: &[u32]) -> CounterValue {
    return CounterValue {
      kind: CounterKind::Theorem(class),
      parts: parts.to_vec(),
    };
  }

  #[test]
  fn chapter_uses_arabic_number_only() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Chapter, &[0, 1]));

    // Assert
    assert_eq!(text, "1");
  }

  #[test]
  fn part_uses_roman_upper_number_style() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Part, &[2]));

    // Assert
    assert_eq!(text, "II");
  }

  #[test]
  fn section_embeds_ancestor_chapter_value() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Section, &[0, 1, 2]));

    // Assert
    assert_eq!(text, "1.2", "既定の section は number_format = \"{{chapter}}.{{n}}\"");
  }

  #[test]
  fn subsection_embeds_two_ancestor_values() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Subsection, &[0, 1, 2, 3]));

    // Assert
    assert_eq!(text, "1.2.3");
  }

  #[test]
  fn literal_decoration_in_number_format_is_kept() {
    // Arrange
    let mut style = Style::default();
    style.counters.chapter.number_format = CounterTemplate::parse("第{n}章");

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Chapter, &[0, 3]));

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
    let text = format_counter_value(&style, &counter_value(CounterName::Chapter, &[2, 1]));

    // Assert
    assert_eq!(text, "II-1", "祖先 part は part 自身の number_style で描画される");
  }

  #[test]
  fn chapter_ref_display_applies_ref_format() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_ref_display(&style, &counter_value(CounterName::Chapter, &[0, 1]));

    // Assert
    assert_eq!(text, "Chapter 1");
  }

  #[test]
  fn equation_ref_display_uses_parenthesized_ref_format() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_ref_display(&style, &counter_value(CounterName::Equation, &[0, 1, 1]));

    // Assert
    assert_eq!(text, "(1.1)");
  }

  #[test]
  fn theorem_number_uses_plain_own_value() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_counter_value(&style, &theorem_value(TheoremClass::Theorem, &[2]));

    // Assert
    assert_eq!(text, "2");
  }

  #[test]
  fn theorem_number_embeds_reset_by_counter() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = CounterTemplate::parse("{section}.{n}");

    // Act
    let text = format_counter_value(&style, &theorem_value(TheoremClass::Theorem, &[3, 1]));

    // Assert
    assert_eq!(text, "3.1");
  }

  #[test]
  fn theorem_ref_display_uses_display_name_and_number() {
    // Arrange
    let style = Style::default();

    // Act
    let text = format_ref_display(&style, &theorem_value(TheoremClass::Lemma, &[4]));

    // Assert
    assert_eq!(text, "Lemma 4");
  }

  #[test]
  fn counter_not_on_ancestor_chain_renders_empty_placeholder() {
    // Arrange — 図の既定の祖先は chapter なので、section の値は構造値に含まれない
    let mut style = Style::default();
    style.counters.figure.number_format = CounterTemplate::parse("{section}.{n}");

    // Act
    let text = format_counter_value(&style, &counter_value(CounterName::Figure, &[0, 1, 5]));

    // Assert
    assert_eq!(text, ".5", "復元できない他カウンタ参照は空文字列になる（既知の制限）");
  }

  #[test]
  fn counter_chain_matches_resolve_ancestor_order() {
    // Arrange
    let counters = Counters::default();

    // Act
    let chain = counter_chain(&counters, CounterName::Subsection);

    // Assert
    assert_eq!(
      chain,
      vec![
        CounterName::Part,
        CounterName::Chapter,
        CounterName::Section,
        CounterName::Subsection,
      ],
      "祖先が先・自身が末尾（resolve::CounterValue::parts と同じ順序）"
    );
  }
}
