//! G3（内容は見た目から独立）の直接検証: 表示だけが異なる style を差し替えても
//! `ResolvedDocument` は同一になるはずのプロパティテスト
#![allow(clippy::unwrap_used)]

use config::Style;
use model::{DocNode, HeadingLevel, InlineNode, Origin, SourceId, Span};
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
      origin: Origin::Source(SourceId::new(0)),
    }],
  };
  return resolve_project(&semantic, style).expect("サンプルは解決に成功するはず");
}

#[test]
fn resolved_document_is_identical_across_display_only_style_variants() {
  // Arrange: 表示側フィールド（number_format / ref_format / display_name）だけが異なる
  // 3 通りの style を用意する。resets（値側フィールド）はいずれも既定のまま変えない。
  let nodes = sample_nodes();
  let base = Style::default();

  let mut different_number_format = Style::default();
  different_number_format.counters.chapter.number_format = "第{n}章".to_string();
  different_number_format.counters.section.number_format = "{chapter}-{n}".to_string();

  let mut different_ref_format = Style::default();
  different_ref_format.counters.chapter.ref_format = "{display_name}（{number}）".to_string();

  let mut different_display_name = Style::default();
  different_display_name.counters.chapter.display_name = "章".to_string();

  // Act
  let base_resolved = resolve_sample(&nodes, &base);
  let variants = [
    resolve_sample(&nodes, &different_number_format),
    resolve_sample(&nodes, &different_ref_format),
    resolve_sample(&nodes, &different_display_name),
  ];

  // Assert: ResolvedDocument（構造値・LabelId・headings すべて）は表示側 style を
  // 変えても完全に同一になる（G3 の直接検証）
  for variant in &variants {
    assert_eq!(&base_resolved, variant, "表示のみ異なる style で ResolvedDocument が変わってはいけない");
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
