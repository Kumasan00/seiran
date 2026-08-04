//! 未解決の名前（ラベル名・`\ref` 参照名・引用キー・索引語）を保持できる `SemanticDocument` と、
//! それらが typed ID へ解決済みの `ResolvedDocument` を分離するクレート。
//!
//! citation クレートの後・typeset クレートの前で実行する。カウンタの値（構造）算出もここに
//! 閉じ、表示文字列の生成（`number_format` 等 style 依存）は typeset 側に残す。

mod counter;
mod document;
mod error;
mod inline;
mod node;
mod resolver;
mod validate;

pub use counter::{CounterKind, CounterValue};
pub use document::{ResolvedDocument, ResolvedGroup, ResolvedHeading, SemanticDocument, SemanticGroup};
pub use error::ResolveError;
// `IndexKey` は旧 resolve crate の公開 API 保持のための再エクスポートで、crate::resolve root
// 経由の利用者は `crate::typeset::lowering::inline` の `#[cfg(test)]` テストのみ。cfg(test) を
// 有効化しない通常ビルドでは未使用に見えるため、この pub use にだけ抑制を付ける。
#[allow(unused_imports)]
pub use inline::{IndexKey, ResolvedInline};
// `ResolvedProofTarget` は seiran 内に crate::resolve root 経由の利用者が現状なく（resolve 内部の
// resolver/validate からのみ参照）、`ResolvedTableCell` は `crate::typeset::lowering::table` の
// `#[cfg(test)]` テストのみが利用する。いずれも通常ビルドでは未使用に見えるため、この pub use に
// だけ抑制を付ける（API 面の維持のため再エクスポート自体は残す）。
#[allow(unused_imports)]
pub use node::{
  ResolvedListItem, ResolvedMathRow, ResolvedNode, ResolvedProofTarget, ResolvedTableCell, ResolvedTableRow,
};

/// `SemanticDocument` を `ResolvedDocument` へ解決する
///
/// 全ソースグループを 1 つの `CounterRegistry` で通しで解決してから（pass1）、
/// 全体に対して `\ref` の存在検証を行う（pass2）。カウンタ・ラベルの登録はソース間で
/// 共有されるため、`\ref` は自ソースだけでなく他ソースのラベルも参照できる。
///
/// # Errors
///
/// いずれかのグループの変換（重複ラベル・未整形の `\cite`）、または `\ref` /
/// `Theorem::of` の存在検証で失敗した場合にエラーを返す。
pub fn resolve_project(
  semantic: &SemanticDocument<'_>,
  style: &config::Style,
) -> Result<ResolvedDocument, ResolveError> {
  let mut registry = counter::CounterRegistry::from_style(style);
  let mut pending_headings = Vec::new();

  let mut groups = Vec::with_capacity(semantic.groups.len());
  for group in &semantic.groups {
    let origin = model::Origin::Source(group.source_id);
    let nodes = resolver::resolve_group(group.nodes, &mut registry, &mut pending_headings, origin)?;
    groups.push(ResolvedGroup {
      nodes,
      source_id: group.source_id,
    });
  }
  let bibliography_origin = model::Origin::Generated(model::GeneratedOrigin::Bibliography);
  let bibliography =
    resolver::resolve_group(semantic.bibliography, &mut registry, &mut pending_headings, bibliography_origin)?;

  for group in &groups {
    validate::validate_refs(&group.nodes, &registry, model::Origin::Source(group.source_id))?;
  }
  validate::validate_refs(&bibliography, &registry, bibliography_origin)?;

  let headings = pending_headings
    .into_iter()
    .map(|pending| {
      return ResolvedHeading {
        key: model::HeadingKey::new(pending.index),
        level: pending.level,
        counter_value: pending.counter_value,
        title: pending.title,
        source: pending.source,
      };
    })
    .collect();

  let counter_values = registry.into_counter_values();

  return Ok(ResolvedDocument {
    groups,
    bibliography,
    headings,
    counter_values,
  });
}

#[cfg(test)]
mod tests {
  use model::{HeadingLevel, InlineNode, SourceId, Span};

  use super::*;

  fn labeled_chapter(title: &str, label: &str) -> model::DocNode {
    return model::DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::text(title)],
      label: Some(label.to_string()),
      span: Span::DUMMY,
    };
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn resolve_project_resolves_ref_across_groups() {
    // Arrange
    let g0 = vec![labeled_chapter("Intro", "ch:intro")];
    let g1 = vec![model::DocNode::Paragraph(vec![InlineNode::Ref {
      label: "ch:intro".to_string(),
      span: Span::DUMMY,
    }])];
    let semantic = SemanticDocument {
      groups: vec![
        SemanticGroup {
          nodes: &g0,
          source_id: SourceId::new(0),
        },
        SemanticGroup {
          nodes: &g1,
          source_id: SourceId::new(1),
        },
      ],
      bibliography: &[],
    };

    // Act
    let resolved = resolve_project(&semantic, &config::Style::default()).expect("跨りラベルは解決されるはず");

    // Assert
    assert_eq!(resolved.headings.len(), 1);
    assert!(resolved.counter_values.contains_key(&model::LabelId::new("ch:intro")));
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn resolve_project_reports_unresolved_reference() {
    // Arrange
    let g0 = vec![model::DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: Span::DUMMY,
    }])];
    let semantic = SemanticDocument {
      groups: vec![SemanticGroup {
        nodes: &g0,
        source_id: SourceId::new(0),
      }],
      bibliography: &[],
    };

    // Act
    let err = resolve_project(&semantic, &config::Style::default()).unwrap_err();

    // Assert
    assert!(matches!(err, ResolveError::UnresolvedReference { ref label, .. } if label == "missing"));
  }
}

/// G3（内容は見た目から独立）の直接検証: 表示だけが異なる style を差し替えても
/// `ResolvedDocument` は同一になるはずのプロパティテスト（旧 `resolve` crate の
/// `tests/style_independence.rs`、#307 で本 module 直下の inline テストへ移設）
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod style_independence_tests {
  use config::Style;
  use model::{DocNode, HeadingLevel, InlineNode, SourceId, Span};
  use proptest::prelude::*;

  use super::{ResolvedDocument, SemanticDocument, SemanticGroup, resolve_project};

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
  fn resolve_sample(nodes: &[DocNode], style: &Style) -> ResolvedDocument {
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

  /// 上書き対象のカウンタ（`sample_nodes` がラベル付きで両方使っている 2 種）。
  #[derive(Debug, Clone, Copy)]
  enum CounterTarget {
    Chapter,
    Section,
  }

  /// 表示専用フィールドのランダムな上書き値。
  ///
  /// 対象は上の手書きテスト（`resolved_document_is_identical_across_display_only_style_variants`）が
  /// 個別に確認している 4 フィールド（`number_format` / `ref_format` / `display_name` /
  /// `number_style`）と同じ集合。value-affecting なフィールド（`resets` 等）は意図的に含めない
  /// （そちらは `resolved_document_differs_when_value_affecting_style_changes` の対象）。
  /// `CounterTarget` と組み合わせて `chapter` / `section` のどちらに適用するかもランダムに選ぶ。
  #[derive(Debug, Clone)]
  enum DisplayOnlyVariant {
    NumberFormat(String),
    RefFormat(String),
    DisplayName(String),
    NumberStyle(config::NumberStyle),
  }

  /// `DisplayOnlyVariant` 1 件を `style.counters.{chapter,section}` の指定先に適用する。
  fn apply_display_only_variant(style: &mut Style, target: CounterTarget, variant: DisplayOnlyVariant) {
    let counter = match target {
      CounterTarget::Chapter => &mut style.counters.chapter,
      CounterTarget::Section => &mut style.counters.section,
    };
    match variant {
      DisplayOnlyVariant::NumberFormat(value) => counter.number_format = value,
      DisplayOnlyVariant::RefFormat(value) => counter.ref_format = value,
      DisplayOnlyVariant::DisplayName(value) => counter.display_name = value,
      DisplayOnlyVariant::NumberStyle(value) => counter.number_style = value,
    }
  }

  /// `CounterTarget` の proptest 戦略（chapter / section を等確率で選ぶ）
  fn counter_target_strategy() -> impl Strategy<Value = CounterTarget> {
    return prop_oneof![Just(CounterTarget::Chapter), Just(CounterTarget::Section)];
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
    /// `chapter` / `section` いずれかのカウンタへ 0〜4 個ランダムに組み合わせて上書きしても、
    /// `resolve_project` の結果（ラベル・typed ID・`CounterValue` を含む `ResolvedDocument`）は
    /// base と一致する（G3 の property test 版。上の手書き 4 ケーステストを任意の組み合わせ・
    /// 対象カウンタへ一般化する）。
    #[test]
    fn resolved_document_is_identical_for_any_display_only_variant_combination(
      variants in prop::collection::vec((counter_target_strategy(), display_only_variant_strategy()), 0..=4),
    ) {
      // Arrange
      let nodes = sample_nodes();
      let base_style = Style::default();
      let mut varied_style = base_style.clone();
      for (target, variant) in variants {
        apply_display_only_variant(&mut varied_style, target, variant);
      }

      // Act
      let base = resolve_sample(&nodes, &base_style);
      let varied = resolve_sample(&nodes, &varied_style);

      // Assert
      prop_assert_eq!(base, varied);
    }
  }
}
