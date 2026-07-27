//! 番号カウンタとラベル登録のレジストリ

use std::collections::HashMap;

use config::{CounterName, Counters, Style, TheoremClass, TheoremReset, Theorems};
use model::{HeadingLevel, LabelId, Origin};

use super::LoweringError;
use crate::layout::Page;

mod format;

use format::expand_ref_format;

/// 確定したページ列から、脚注のページ単位表示番号を割り当てる（`FootnoteNumbering::PerPage`）
///
/// # Panics
///
/// 文書中の脚注数、または 1 ページの脚注数が `u32` に収まらない場合にパニックします。
#[must_use]
pub fn per_page_footnote_numbers(pages: &[Page]) -> Vec<u32> {
  let mut numbers: Vec<u32> = Vec::new();
  for page in pages {
    // 繰越（前ページからの続き、#227）はこのページで「始まった」脚注ではないので数えない。
    // 数えると (a) 自分自身が本体を置いた前ページの番号を上書きし、(b) このページの本当の
    // 1 個目を 2 番へずらす。
    for (position, footnote) in page.footnotes.iter().filter(|footnote| return !footnote.continued).enumerate() {
      let index = usize::try_from(footnote.index).expect("脚注の出現 index は usize に収まる前提");
      // 目的の index まで通し値で伸ばしてから、配置が分かっている脚注だけを上書きする。
      // 伸ばした途中の穴（未配置の脚注）は通し値のまま残る。
      while numbers.len() <= index {
        let continuous = u32::try_from(numbers.len() + 1).expect("脚注の個数は u32 に収まる前提");
        numbers.push(continuous);
      }
      numbers[index] = u32::try_from(position + 1).expect("1 ページの脚注数は u32 に収まる前提");
    }
  }
  return numbers;
}

/// カウンタ群の状態と labels の登録状態を保持するレジストリ
#[derive(Debug, Clone)]
pub(crate) struct CounterRegistry {
  /// カウンタ定義（`config::Counters` の複製）
  defs: Counters,
  /// 各カウンタの現在値。未登場のカウンタは 0 とみなす
  values: HashMap<CounterName, u32>,
  /// 定理クラス定義（`config::Theorems` の複製）。共有カウンタ名・リセット先・番号書式を引く
  theorems: Theorems,
  /// 定理カウンタの現在値。キーは共有カウンタ名（`TheoremStyle.counter`）。未登場は 0
  theorem_values: HashMap<String, u32>,
  /// `\ref` 解決用テーブル。pass1 で登録、pass2 で参照する
  labels: HashMap<LabelId, ResolvedLabel>,
  /// これまでに発番した脚注の個数（次の脚注の出現 index になる）
  footnote_count: u32,
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
      footnote_count: 0,
    };
  }

  /// 指定カウンタを 1 増やし、リセット連鎖を実行し、書式化済みの番号文字列を返す
  pub(crate) fn increment(&mut self, name: CounterName) -> String {
    *self.values.entry(name).or_insert(0) += 1;
    for r in &self.defs.get(name).resets {
      self.values.insert(*r, 0);
    }
    if let Some(level) = theorem_reset_level(name) {
      self.reset_theorems_for_level(level);
    }

    return self.format_number(name);
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

  /// 定理環境を採番し、`label` があれば cleveref 形式で登録する
  ///
  /// # Errors
  ///
  /// `label` が既に登録済みの場合に [`LoweringError::DuplicateLabel`] を返します。
  pub(crate) fn increment_theorem_with_label(
    &mut self,
    class: TheoremClass,
    label: Option<&str>,
    span: model::Span,
    source: Origin,
  ) -> Result<Option<String>, LoweringError> {
    // def への借用を必要なクローンに落としてから theorem_values を変更する
    let (counter, number_format, display_name, unnumbered) = {
      let def = self.theorems.get(class);
      (def.counter.clone(), def.number_format.clone(), def.display_name.clone(), def.unnumbered)
    };
    if unnumbered {
      return Ok(None);
    }

    let value = {
      let v = self.theorem_values.entry(counter).or_insert(0);
      *v += 1;
      *v
    };
    let number = self.expand_theorem_template(&number_format, value);

    if let Some(l) = label {
      let formatted = expand_ref_format("{display_name} {number}", &number, &display_name);
      if !self.register_formatted_label(l.to_string(), formatted) {
        return Err(LoweringError::DuplicateLabel {
          label: l.to_string(),
          span: super::span_to_source_span(span),
          origin: source,
        });
      }
    }
    return Ok(Some(number));
  }

  /// 脚注を 1 つ数え、その出現 index（0 起点）を返す（ラベル解決は不要なので単純増加のみ）
  pub(crate) fn next_footnote_index(&mut self) -> u32 {
    let index = self.footnote_count;
    self.footnote_count += 1;
    return index;
  }

  /// 現在のカウンタ値を `number_format` テンプレートに従って書式化する
  #[must_use]
  pub(crate) fn format_number(&self, name: CounterName) -> String {
    return self.expand_template(&self.defs.get(name).number_format, name);
  }

  /// カウンタの現在値を返す（未登場のカウンタは 0）
  fn value(&self, name: CounterName) -> u32 { return self.values.get(&name).copied().unwrap_or(0); }

  /// テンプレートのプレースホルダ `{n}` / `{<counter_name>}` を値で置換する
  fn expand_template(&self, template: &str, self_name: CounterName) -> String {
    return super::placeholder::expand(template, |name| {
      let target = if name == "n" {
        Some(self_name)
      } else {
        CounterName::from_name(name)
      };
      return target.map_or_else(String::new, |t| return self.render_counter_value(t));
    });
  }

  /// 定理の番号テンプレート（`"{n}"` / `"{chapter}.{n}"` 等）を値で置換する
  fn expand_theorem_template(&self, template: &str, self_value: u32) -> String {
    return super::placeholder::expand(template, |name| {
      if name == "n" {
        return self_value.to_string();
      }
      return CounterName::from_name(name).map_or_else(String::new, |t| return self.render_counter_value(t));
    });
  }

  /// カウンタの「現在値を自身の `number_style` で描画した文字列」を返す
  fn render_counter_value(&self, name: CounterName) -> String {
    return self.defs.get(name).number_style.render(self.value(name));
  }

  /// pass1 で `\section[label=sec:intro]{...}` などからラベルを登録する
  #[must_use]
  pub(crate) fn register_label(
    &mut self,
    label: impl Into<LabelId>,
    counter: CounterName,
    number: impl Into<String>,
  ) -> bool {
    let label = label.into();
    if self.labels.contains_key(&label) {
      return false;
    }
    let def = self.defs.get(counter);
    let formatted = expand_ref_format(&def.ref_format, &number.into(), &def.display_name);
    self.labels.insert(label, ResolvedLabel { number: formatted });
    return true;
  }

  /// 整形済みの `\ref` 表示文字列をそのまま登録する（定理の cleveref 用）
  #[must_use]
  fn register_formatted_label(&mut self, label: impl Into<LabelId>, formatted: String) -> bool {
    let label = label.into();
    if self.labels.contains_key(&label) {
      return false;
    }
    self.labels.insert(label, ResolvedLabel { number: formatted });
    return true;
  }

  /// 採番とラベル登録を一括で行う共通処理
  ///
  /// # Errors
  ///
  /// `label` が既に登録済みの場合に [`LoweringError::DuplicateLabel`] を返します。
  pub(crate) fn increment_with_label(
    &mut self,
    counter: CounterName,
    label: Option<&str>,
    span: model::Span,
    source: Origin,
  ) -> Result<String, LoweringError> {
    let number = self.increment(counter);
    if let Some(l) = label
      && !self.register_label(l.to_string(), counter, &number)
    {
      return Err(LoweringError::DuplicateLabel {
        label: l.to_string(),
        span: super::span_to_source_span(span),
        origin: source,
      });
    }
    return Ok(number);
  }

  /// pass2 で `\ref{label}` を解決して番号文字列を返す
  #[must_use]
  pub(crate) fn resolve_label(&self, label: &str) -> Option<&str> {
    return self.labels.get(label).map(|r| return r.number.as_str());
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
      footnote_count: 0,
    };
  }
}

/// pass1 で登録される、ラベル名から確定済み番号への解決結果
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLabel {
  /// 確定済みの `\ref` 表示文字列（カウンタは `ref_format`、定理は cleveref 書式を適用済み）
  number: String,
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

#[cfg(test)]
mod tests {
  use config::{CounterName, CounterStyle, Counters, NumberStyle, Style, TheoremClass, TheoremReset};
  use model::Span;

  use super::*;
  use crate::layout::PlacedFootnote;

  fn theorem_span() -> Span { return Span::DUMMY; }

  /// 指定した出現 index の脚注だけを載せたページを作るテストヘルパ
  fn page_with_footnotes(indices: &[u32]) -> Page {
    return page_with_footnote_fragments(&indices.iter().map(|index| return (*index, false)).collect::<Vec<_>>());
  }

  /// 出現 index と「繰越（前ページからの続き）か」の組から 1 ページを作るテストヘルパ
  fn page_with_footnote_fragments(fragments: &[(u32, bool)]) -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: fragments
        .iter()
        .map(|(index, continued)| {
          return PlacedFootnote {
            number: index + 1,
            index: *index,
            continued: *continued,
            blocks: Vec::new(),
          };
        })
        .collect(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
    };
  }

  #[test]
  fn per_page_numbers_restart_from_one_on_each_page() {
    // Arrange
    let pages = vec![
      page_with_footnotes(&[0, 1, 2]),
      page_with_footnotes(&[3, 4]),
    ];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 2, 3, 1, 2]);
  }

  #[test]
  fn per_page_numbers_restart_after_page_without_footnotes() {
    // Arrange
    let pages = vec![
      page_with_footnotes(&[0]),
      page_with_footnotes(&[]),
      page_with_footnotes(&[1]),
    ];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 1]);
  }

  #[test]
  fn per_page_numbers_ignore_carried_over_fragments() {
    // Arrange
    let pages = vec![
      page_with_footnote_fragments(&[(0, false)]),
      page_with_footnote_fragments(&[(0, true), (1, false)]),
    ];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 1]);
  }

  #[test]
  fn per_page_numbers_fill_unplaced_footnotes_with_continuous_value() {
    // Arrange
    let pages = vec![page_with_footnotes(&[0, 2])];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 2, 2]);
  }

  #[test]
  fn per_page_numbers_are_empty_without_footnotes() {
    // Arrange / Act
    let numbers = per_page_footnote_numbers(&[page_with_footnotes(&[])]);

    // Assert
    assert!(numbers.is_empty());
  }

  #[test]
  fn increment_theorem_numbers_with_default_format() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act / Assert
    assert_eq!(
      r.increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0))
      )
      .unwrap()
      .as_deref(),
      Some("1")
    );
    assert_eq!(
      r.increment_theorem_with_label(
        TheoremClass::Lemma,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0))
      )
      .unwrap()
      .as_deref(),
      Some("2")
    );
  }

  #[test]
  fn increment_theorem_proof_is_unnumbered() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act / Assert
    assert!(
      r.increment_theorem_with_label(
        TheoremClass::Proof,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0))
      )
      .unwrap()
      .is_none()
    );
  }

  #[test]
  fn increment_theorem_definition_uses_separate_counter() {
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
      .unwrap();
    let def = r
      .increment_theorem_with_label(
        TheoremClass::Definition,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();

    // Assert
    assert_eq!(thm.as_deref(), Some("1"));
    assert_eq!(def.as_deref(), Some("1"));
  }

  #[test]
  fn theorem_format_embeds_section_counter() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.number_format = "{section}.{n}".to_string();
    let mut r = CounterRegistry::from_style(&style);

    // Act
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section);
    let number = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();

    // Assert
    assert_eq!(number.as_deref(), Some("1.1"));
  }

  #[test]
  fn theorem_counter_resets_on_reset_by_heading() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    style.theorems.theorem.number_format = "{section}.{n}".to_string();
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
      .unwrap();
    let b = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();
    r.increment(CounterName::Section); // section = 2、theorem カウンタは 0 にリセット
    let c = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();

    // Assert
    assert_eq!(a.as_deref(), Some("1.1"));
    assert_eq!(b.as_deref(), Some("1.2"));
    assert_eq!(c.as_deref(), Some("2.1"));
  }

  #[test]
  fn theorem_counter_not_reset_by_unrelated_heading() {
    // Arrange
    let mut style = Style::default();
    style.theorems.theorem.reset_by = TheoremReset::Section;
    let mut r = CounterRegistry::from_style(&style);
    r.increment(CounterName::Chapter); // chapter = 1

    // Act
    let a = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();
    r.increment(CounterName::Chapter); // chapter = 2（section は増えていない）
    let b = r
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        None,
        theorem_span(),
        Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();

    // Assert
    assert_eq!(a.as_deref(), Some("1"));
    assert_eq!(b.as_deref(), Some("2"));
  }

  #[test]
  fn increment_theorem_registers_cleveref_label() {
    // Arrange
    let mut r = CounterRegistry::from_style(&Style::default());

    // Act
    r.increment_theorem_with_label(
      TheoremClass::Theorem,
      Some("thm:x"),
      theorem_span(),
      Origin::Source(model::SourceId::new(0)),
    )
    .unwrap();

    // Assert
    assert_eq!(r.resolve_label("thm:x"), Some("Theorem 1"));
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
    assert!(matches!(result, Err(LoweringError::DuplicateLabel { ref label, .. }) if label == "dup"));
  }

  #[test]
  fn counter_registry_increment_format() {
    let mut r = CounterRegistry::default_for_seiran();
    assert_eq!(r.increment(CounterName::Chapter), "1");
    assert_eq!(r.increment(CounterName::Section), "1.1");
    assert_eq!(r.increment(CounterName::Section), "1.2");
    assert_eq!(r.format_number(CounterName::Chapter), "1");
    assert_eq!(r.format_number(CounterName::Section), "1.2");
  }

  #[test]
  fn counter_registry_part_uses_roman_upper() {
    let mut r = CounterRegistry::default_for_seiran();
    assert_eq!(r.increment(CounterName::Part), "I");
    assert_eq!(r.increment(CounterName::Part), "II");
  }

  #[test]
  fn counter_registry_section_reset() {
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter); // chapter = 1
    r.increment(CounterName::Section); // section = 1.1
    r.increment(CounterName::Section); // section = 1.2
    r.increment(CounterName::Chapter); // chapter = 2、section は 0 にリセット
    let next = r.increment(CounterName::Section);
    assert_eq!(next, "2.1");
  }

  #[test]
  fn template_with_literal_decoration() {
    // Arrange
    let counters = Counters {
      chapter: CounterStyle {
        display_name: "Chapter".to_string(),
        number_format: "第{n}章".to_string(),
        number_style: NumberStyle::Arabic,
        ref_format: "{number}".to_string(),
        resets: vec![],
      },
      ..Counters::default()
    };
    let mut r = CounterRegistry::from_counters(&counters);

    // Act / Assert
    assert_eq!(r.increment(CounterName::Chapter), "第1章");
    assert_eq!(r.increment(CounterName::Chapter), "第2章");
  }

  #[test]
  fn template_cross_counter_uses_target_number_style() {
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
    assert_eq!(ch, "II-1");
  }

  #[test]
  fn evaluate_ref_pass2_applies_ref_format() {
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter);
    let bare = r.format_number(CounterName::Chapter);
    assert!(r.register_label("ch:intro", CounterName::Chapter, bare));

    assert_eq!(r.resolve_label("ch:intro"), Some("Chapter 1"));
  }

  #[test]
  fn evaluate_ref_equation_uses_parenthesized_ref_format() {
    let mut r = CounterRegistry::default_for_seiran();
    r.increment(CounterName::Chapter);
    let bare = r.increment(CounterName::Equation);
    assert!(r.register_label("eq:foo", CounterName::Equation, bare));

    assert_eq!(r.resolve_label("eq:foo"), Some("(1.1)"));
  }

  #[test]
  fn evaluate_unknown_label_errors() {
    let r = CounterRegistry::default_for_seiran();
    assert!(r.resolve_label("nonexistent").is_none());
  }

  #[test]
  fn from_style_with_default_style_matches_default_for_seiran() {
    // Arrange / Act
    let mut from_default = CounterRegistry::from_style(&Style::default());
    let mut from_helper = CounterRegistry::default_for_seiran();

    // Assert
    assert_eq!(from_default.increment(CounterName::Chapter), from_helper.increment(CounterName::Chapter));
    assert_eq!(from_default.increment(CounterName::Section), from_helper.increment(CounterName::Section));
  }

  #[test]
  fn next_footnote_index_returns_sequential_indices() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();

    // Act / Assert
    assert_eq!(r.next_footnote_index(), 0);
    assert_eq!(r.next_footnote_index(), 1);
    assert_eq!(r.next_footnote_index(), 2);
  }

  #[test]
  fn next_footnote_index_unaffected_by_unrelated_counters() {
    // Arrange
    let mut r = CounterRegistry::default_for_seiran();
    r.next_footnote_index(); // 0

    // Act
    r.increment(CounterName::Chapter);
    r.increment(CounterName::Section);
    let next = r.next_footnote_index();

    // Assert
    assert_eq!(next, 1);
  }

  #[test]
  fn counter_name_for_heading_maps_each_level() {
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Part), CounterName::Part);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Chapter), CounterName::Chapter);
    assert_eq!(CounterRegistry::counter_name_for_heading(HeadingLevel::Subparagraph), CounterName::Subparagraph);
  }
}
