//! G3（内容は見た目から独立）の直接検証: 表示だけが異なる style を差し替えても
//! `ResolvedDocument` は同一になるはずのプロパティテスト
#![allow(clippy::unwrap_used)]

use config::Style;
use model::{DocNode, HeadingLevel, InlineNode, SourceId, Span};
use proptest::prelude::*;
use resolve::{SemanticDocument, SemanticGroup, resolve_project};

/// 見出し・本文・`\ref` を含む代表的なドキュメントを組み立てる
fn sample_nodes() -> Vec<DocNode> {
  return vec![
    DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::text("Intro")],
      label: Some("ch:intro".to_string()),
      span: Span::DUMMY,
    },
    DocNode::Heading {
      level: HeadingLevel::Section,
      numbered: true,
      title: vec![InlineNode::text("Details")],
      label: Some("sec:details".to_string()),
      span: Span::DUMMY,
    },
    DocNode::Paragraph(vec![
      InlineNode::Text("See ".to_string()),
      InlineNode::Ref {
        label: "ch:intro".to_string(),
        span: Span::DUMMY,
      },
      InlineNode::Text(" and ".to_string()),
      InlineNode::Ref {
        label: "sec:details".to_string(),
        span: Span::DUMMY,
      },
      InlineNode::Text(".".to_string()),
    ]),
  ];
}

/// `nodes` を 1 ソースグループとして解決する
fn resolve_sample(nodes: &[DocNode], style: &Style) -> resolve::ResolvedDocument {
  let semantic = SemanticDocument {
    groups: vec![SemanticGroup {
      nodes,
      source_id: SourceId::new(0),
    }],
    bibliography: &[],
  };
  return resolve_project(&semantic, style).expect("サンプルは解決に成功するはず");
}

#[test]
fn resolved_document_is_identical_across_display_only_style_variants() {
  // Arrange: 表示側フィールド（number_format / ref_format / display_name / number_style）だけが
  // 異なる 4 通りの style を用意する。resets（値側フィールド）はいずれも既定のまま変えない。
  let nodes = sample_nodes();
  let base = Style::default();

  let mut different_number_format = Style::default();
  different_number_format.counters.chapter.number_format = "第{n}章".to_string();
  different_number_format.counters.section.number_format = "{chapter}-{n}".to_string();

  let mut different_ref_format = Style::default();
  different_ref_format.counters.chapter.ref_format = "{display_name}（{number}）".to_string();

  let mut different_display_name = Style::default();
  different_display_name.counters.chapter.display_name = "章".to_string();

  let mut different_number_style = Style::default();
  different_number_style.counters.chapter.number_style = config::NumberStyle::RomanUpper;

  // Act
  let base_resolved = resolve_sample(&nodes, &base);
  let variants = [
    ("number_format", resolve_sample(&nodes, &different_number_format)),
    ("ref_format", resolve_sample(&nodes, &different_ref_format)),
    ("display_name", resolve_sample(&nodes, &different_display_name)),
    ("number_style", resolve_sample(&nodes, &different_number_style)),
  ];

  // Assert: ResolvedDocument（構造値・LabelId・headings すべて）は表示側 style を
  // 変えても完全に同一になる（G3 の直接検証）。失敗時にどの variant が壊れたか
  // 分かるようラベル付きでメッセージに含める。
  for (label, variant) in &variants {
    assert_eq!(
      &base_resolved, variant,
      "表示のみ異なる style（variant: {label}）で ResolvedDocument が変わってはいけない"
    );
  }
}

#[test]
fn resolved_document_differs_when_value_affecting_style_changes() {
  // Arrange: resets（値側フィールド）を変えた style
  let nodes = vec![
    DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::text("A")],
      label: None,
      span: Span::DUMMY,
    },
    DocNode::Heading {
      level: HeadingLevel::Section,
      numbered: true,
      title: vec![InlineNode::text("A.1")],
      label: Some("sec:a1".to_string()),
      span: Span::DUMMY,
    },
    DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::text("B")],
      label: None,
      span: Span::DUMMY,
    },
    DocNode::Heading {
      level: HeadingLevel::Section,
      numbered: true,
      title: vec![InlineNode::text("B.1")],
      label: Some("sec:b1".to_string()),
      span: Span::DUMMY,
    },
  ];
  let base = Style::default();
  let mut reset_variant = Style::default();
  reset_variant.counters.chapter.resets = vec![]; // section が chapter で 0 リセットされなくなる

  // Act
  let base_resolved = resolve_sample(&nodes, &base);
  let reset_resolved = resolve_sample(&nodes, &reset_variant);

  // Assert: 2 番目の section（sec:b1）の CounterValue が resets の有無で変わる
  let base_value = base_resolved.counter_values.get(&model::LabelId::new("sec:b1")).expect("登録済みのはず");
  let reset_value = reset_resolved.counter_values.get(&model::LabelId::new("sec:b1")).expect("登録済みのはず");
  assert_ne!(base_value, reset_value, "resets は値側フィールドなので CounterValue に影響するはず");
}

/// 表示専用フィールドのランダムな上書き値。
///
/// 対象は上の手書きテスト（`resolved_document_is_identical_across_display_only_style_variants`）が
/// 個別に確認している 4 フィールド（`style.counters.chapter` の `number_format` / `ref_format` /
/// `display_name` / `number_style`）と同じ集合。value-affecting なフィールド（`resets` 等）は
/// 意図的に含めない（そちらは `resolved_document_differs_when_value_affecting_style_changes` の対象）。
#[derive(Debug, Clone)]
enum DisplayOnlyVariant {
  NumberFormat(String),
  RefFormat(String),
  DisplayName(String),
  NumberStyle(config::NumberStyle),
}

/// `DisplayOnlyVariant` 1 件を `style.counters.chapter` に適用する。
fn apply_display_only_variant(style: &mut Style, variant: DisplayOnlyVariant) {
  match variant {
    DisplayOnlyVariant::NumberFormat(value) => style.counters.chapter.number_format = value,
    DisplayOnlyVariant::RefFormat(value) => style.counters.chapter.ref_format = value,
    DisplayOnlyVariant::DisplayName(value) => style.counters.chapter.display_name = value,
    DisplayOnlyVariant::NumberStyle(value) => style.counters.chapter.number_style = value,
  }
}

/// `config::NumberStyle` の全 variant を等確率で選ぶ戦略
fn number_style_strategy() -> impl Strategy<Value = config::NumberStyle> {
  return prop_oneof![
    Just(config::NumberStyle::Arabic),
    Just(config::NumberStyle::RomanUpper),
    Just(config::NumberStyle::RomanLower),
    Just(config::NumberStyle::AlphaUpper),
    Just(config::NumberStyle::AlphaLower),
    Just(config::NumberStyle::Kanji),
  ];
}

/// `DisplayOnlyVariant` の proptest 戦略
fn display_only_variant_strategy() -> impl Strategy<Value = DisplayOnlyVariant> {
  return prop_oneof![
    "[a-z]{1,8}".prop_map(DisplayOnlyVariant::NumberFormat),
    "[a-z]{1,8}".prop_map(DisplayOnlyVariant::RefFormat),
    "[a-z]{1,8}".prop_map(DisplayOnlyVariant::DisplayName),
    number_style_strategy().prop_map(DisplayOnlyVariant::NumberStyle),
  ];
}

proptest! {
  /// #306: 表示専用フィールド（`number_format` / `ref_format` / `display_name` / `number_style`）を
  /// 0〜4 個ランダムに組み合わせて上書きしても、`resolve_project` の結果（ラベル・typed ID・
  /// `CounterValue` を含む `ResolvedDocument`）は base と一致する（G3 の property test 版。上の
  /// 手書き 4 ケーステストを任意の組み合わせへ一般化する）。
  #[test]
  fn resolved_document_is_identical_for_any_display_only_variant_combination(
    variants in prop::collection::vec(display_only_variant_strategy(), 0..=4),
  ) {
    // Arrange
    let nodes = sample_nodes();
    let base_style = Style::default();
    let mut varied_style = base_style.clone();
    for variant in variants {
      apply_display_only_variant(&mut varied_style, variant);
    }

    // Act
    let base = resolve_sample(&nodes, &base_style);
    let varied = resolve_sample(&nodes, &varied_style);

    // Assert
    prop_assert_eq!(base, varied);
  }
}
