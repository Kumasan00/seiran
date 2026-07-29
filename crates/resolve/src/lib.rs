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
pub use inline::{IndexKey, ResolvedInline};
pub use node::{ResolvedListItem, ResolvedMathRow, ResolvedNode, ResolvedTableCell, ResolvedTableRow};

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
    let nodes = resolver::resolve_group(group.nodes, &mut registry, &mut pending_headings, group.origin)?;
    groups.push(ResolvedGroup {
      nodes,
      origin: group.origin,
    });
  }

  for group in &groups {
    validate::validate_refs(&group.nodes, &registry, group.origin)?;
  }

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
    headings,
    counter_values,
  });
}

#[cfg(test)]
mod tests {
  use model::{HeadingLevel, InlineNode, Origin, SourceId, Span};

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
          origin: Origin::Source(SourceId::new(0)),
        },
        SemanticGroup {
          nodes: &g1,
          origin: Origin::Source(SourceId::new(1)),
        },
      ],
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
        origin: Origin::Source(SourceId::new(0)),
      }],
    };

    // Act
    let err = resolve_project(&semantic, &config::Style::default()).unwrap_err();

    // Assert
    assert!(matches!(err, ResolveError::UnresolvedReference { ref label, .. } if label == "missing"));
  }
}
