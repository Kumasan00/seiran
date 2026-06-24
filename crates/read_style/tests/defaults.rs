//! `Style::default()` および各サブ型の既定値が `validate` を通ることの確認。
//!
//! 各 struct 内 `mod tests` の `validate_accepts_default` と重複するが、トップレベルの
//! Style とサブ型の組合せが矛盾しないことを保証する統合確認として残す。

use garde::Validate;
use read_style::{CounterName, Style, TheoremClass};
use types::HeadingLevel;

#[test]
fn default_style_passes_validation() {
  let style = Style::default();
  assert!(style.validate().is_ok(), "Style::default() must pass garde validation");
}

#[test]
fn default_heading_has_descending_font_size() {
  // 既定値（default_for_level）でレベル順にフォントサイズが単調減少することを確認
  let style = Style::default();
  let part = style.heading(HeadingLevel::Part).font_size.to_pt();
  let chapter = style.heading(HeadingLevel::Chapter).font_size.to_pt();
  let section = style.heading(HeadingLevel::Section).font_size.to_pt();
  let subparagraph = style.heading(HeadingLevel::Subparagraph).font_size.to_pt();
  assert!(part > chapter, "Part should be larger than Chapter: {part} vs {chapter}");
  assert!(chapter > section, "Chapter should be larger than Section: {chapter} vs {section}");
  assert!(section > subparagraph, "Section should be larger than Subparagraph");
}

#[test]
fn default_counters_contains_canonical_set() {
  let style = Style::default();
  for name in [
    CounterName::Part,
    CounterName::Chapter,
    CounterName::Section,
    CounterName::Subsection,
    CounterName::Paragraph,
    CounterName::Subparagraph,
    CounterName::Figure,
    CounterName::Equation,
    CounterName::Table,
  ] {
    assert!(!style.counter(name).display_name.is_empty(), "default counters must contain {}", name.as_str());
  }
}

#[test]
fn default_theorems_all_classes_have_display_name() {
  // 10 種すべてのクラスに非空の表示名が既定で設定されていることを確認
  let style = Style::default();
  for class in TheoremClass::ALL {
    assert!(
      !style.theorem(class).display_name.is_empty(),
      "default theorems must have display_name for {}",
      class.as_str()
    );
  }
}

#[test]
fn default_proof_is_unnumbered_with_qed_mark() {
  let style = Style::default();
  let proof = style.theorem(TheoremClass::Proof);
  assert!(proof.unnumbered, "proof must be unnumbered by default");
  assert!(proof.qed_mark.is_some(), "proof must have a QED mark by default");
}
