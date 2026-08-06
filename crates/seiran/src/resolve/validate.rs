//! pass2: `ResolvedInline::Ref` / `ResolvedNode::Theorem::of` の参照先が実在するかだけを
//! 検証する（ツリーの書き換えは行わない）
//!
//! ラベルは前方参照を許す（`\ref` が指すラベルが文書上その後に定義されうる）ため、
//! 存在確認は pass1（`resolve_group`）が全ラベルの登録を終えた後にしか行えない。
//! これが pass1 とは別の走査（本モジュール）として独立している理由

use crate::{
  model::Origin,
  resolve::{
    ResolveError, counter::CounterRegistry, error::span_to_source_span, inline::ResolvedInline, node::ResolvedNode,
  },
};

/// `nodes`（1 ソースグループぶん）を走査し、すべての `Ref` / `Theorem::of` の参照先が
/// 登録済みか検証する
///
/// # Errors
///
/// 未登録のラベルを参照している場合に [`ResolveError::UnresolvedReference`] を返します。
pub(crate) fn validate_refs(
  nodes: &[ResolvedNode],
  registry: &CounterRegistry,
  origin: Origin,
) -> Result<(), ResolveError> {
  for node in nodes {
    validate_node(node, registry, origin)?;
  }
  return Ok(());
}

/// 単一の `ResolvedNode` を検証する
fn validate_node(node: &ResolvedNode, registry: &CounterRegistry, origin: Origin) -> Result<(), ResolveError> {
  match node {
    ResolvedNode::Heading { title, .. } => validate_inlines(title, registry, origin)?,
    ResolvedNode::Paragraph(inlines) => validate_inlines(inlines, registry, origin)?,
    ResolvedNode::List { items, .. } => {
      for item in items {
        validate_refs(&item.content, registry, origin)?;
      }
    },
    ResolvedNode::Theorem { body, of, .. } => {
      if let Some(of) = of
        && registry.resolve_label(of.target.as_str()).is_none()
      {
        return Err(ResolveError::UnresolvedReference {
          label: of.target.as_str().to_string(),
          span: span_to_source_span(of.span),
          origin,
        });
      }
      validate_refs(body, registry, origin)?;
    },
    ResolvedNode::Quote { body, .. } => validate_refs(body, registry, origin)?,
    ResolvedNode::Figure { caption, .. } => {
      if let Some(caption) = caption {
        validate_inlines(caption, registry, origin)?;
      }
    },
    ResolvedNode::Table {
      caption,
      head,
      rows,
      ..
    } => {
      if let Some(caption) = caption {
        validate_inlines(caption, registry, origin)?;
      }
      for row in head.iter().chain(rows.iter()) {
        for cell in &row.cells {
          validate_inlines(&cell.content, registry, origin)?;
        }
      }
    },
    ResolvedNode::MathBlock { .. }
    | ResolvedNode::Rule { .. }
    | ResolvedNode::PageBreak
    | ResolvedNode::Space(_)
    | ResolvedNode::Anchor(_) => {},
  }
  return Ok(());
}

/// インライン要素列を検証する
fn validate_inlines(
  inlines: &[ResolvedInline],
  registry: &CounterRegistry,
  origin: Origin,
) -> Result<(), ResolveError> {
  for inline in inlines {
    validate_inline(inline, registry, origin)?;
  }
  return Ok(());
}

/// 単一のインライン要素を検証する
fn validate_inline(inline: &ResolvedInline, registry: &CounterRegistry, origin: Origin) -> Result<(), ResolveError> {
  match inline {
    ResolvedInline::Ref { target, span } => {
      if registry.resolve_label(target.as_str()).is_none() {
        return Err(ResolveError::UnresolvedReference {
          label: target.as_str().to_string(),
          span: span_to_source_span(*span),
          origin,
        });
      }
    },
    ResolvedInline::Styled { children, .. }
    | ResolvedInline::Colored { children, .. }
    | ResolvedInline::Link { children, .. }
    | ResolvedInline::InternalLink { children, .. }
    | ResolvedInline::Footnote { body: children, .. } => validate_inlines(children, registry, origin)?,
    // `Cite` は表示を持たない（生成物の side table 側にあり、そこに `\ref` は現れない）
    ResolvedInline::Text(_)
    | ResolvedInline::InlineMath(_)
    | ResolvedInline::Symbol(_)
    | ResolvedInline::LineBreak
    | ResolvedInline::NoIndent
    | ResolvedInline::Cite { .. }
    | ResolvedInline::Index { .. } => {},
  }
  return Ok(());
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    model::{LabelId, Origin, SourceId, Span},
    resolve::{
      counter::CounterRegistry,
      node::{ResolvedListItem, ResolvedProofTarget, ResolvedTableCell, ResolvedTableRow},
    },
  };

  #[allow(clippy::unwrap_used)]
  #[test]
  fn validate_refs_errors_on_missing_label() {
    // Arrange
    let registry = CounterRegistry::default_for_seiran();
    let nodes = vec![ResolvedNode::Paragraph(vec![ResolvedInline::Ref {
      target: LabelId::new("missing"),
      span: Span::DUMMY,
    }])];

    // Act
    let err = validate_refs(&nodes, &registry, Origin::Source(SourceId::new(0))).unwrap_err();

    // Assert
    assert!(matches!(err, ResolveError::UnresolvedReference { ref label, .. } if label == "missing"));
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn validate_refs_passes_when_label_registered() {
    // Arrange
    let mut registry = CounterRegistry::default_for_seiran();
    registry
      .increment_with_label(
        crate::config::CounterName::Chapter,
        Some("ch:intro"),
        Span::DUMMY,
        Origin::Source(SourceId::new(0)),
      )
      .unwrap();
    let nodes = vec![ResolvedNode::Paragraph(vec![ResolvedInline::Ref {
      target: LabelId::new("ch:intro"),
      span: Span::DUMMY,
    }])];

    // Act / Assert
    validate_refs(&nodes, &registry, Origin::Source(SourceId::new(0))).expect("登録済みラベルへの参照は成功するはず");
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn validate_refs_allows_forward_reference_when_run_after_full_pass1() {
    // Arrange: \ref が指すラベルが文書上その後に定義される場合でも、pass1 が全体を
    // 終えた後に pass2 を走らせれば解決できることを確かめる
    let mut registry = CounterRegistry::default_for_seiran();
    let nodes = vec![ResolvedNode::Paragraph(vec![ResolvedInline::Ref {
      target: LabelId::new("ch:later"),
      span: Span::DUMMY,
    }])];
    registry
      .increment_with_label(
        crate::config::CounterName::Chapter,
        Some("ch:later"),
        Span::DUMMY,
        Origin::Source(SourceId::new(0)),
      )
      .unwrap();

    // Act / Assert
    validate_refs(&nodes, &registry, Origin::Source(SourceId::new(0)))
      .expect("pass1 完了後なら前方参照も解決できるはず");
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn validate_refs_errors_on_missing_theorem_of_target() {
    // Arrange
    let registry = CounterRegistry::default_for_seiran();
    let nodes = vec![ResolvedNode::Theorem {
      class: crate::model::TheoremClass::Proof,
      title: None,
      body: Vec::new(),
      of: Some(ResolvedProofTarget {
        target: LabelId::new("thm:missing"),
        span: Span::DUMMY,
      }),
      label: None,
      counter_value: None,
      span: Span::DUMMY,
    }];

    // Act
    let err = validate_refs(&nodes, &registry, Origin::Source(SourceId::new(0))).unwrap_err();

    // Assert
    assert!(matches!(err, ResolveError::UnresolvedReference { ref label, .. } if label == "thm:missing"));
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn validate_refs_reports_of_targets_own_span_not_theorem_span() {
    // Arrange: `[of=...]` 引数のソース位置（span）と定理環境自体のソース位置を
    // 意図的に異なる値にし、未解決診断がどちらの span を報告するかを区別できるようにする
    let registry = CounterRegistry::default_for_seiran();
    let of_span = Span::new(10, 20);
    let theorem_span = Span::new(100, 200);
    let nodes = vec![ResolvedNode::Theorem {
      class: crate::model::TheoremClass::Proof,
      title: None,
      body: Vec::new(),
      of: Some(ResolvedProofTarget {
        target: LabelId::new("thm:missing"),
        span: of_span,
      }),
      label: None,
      counter_value: None,
      span: theorem_span,
    }];

    // Act
    let err = validate_refs(&nodes, &registry, Origin::Source(SourceId::new(0))).unwrap_err();

    // Assert: 報告される span は `of` 引数自身の span であり、定理環境の span ではない
    let ResolveError::UnresolvedReference { span, .. } = err else {
      panic!("UnresolvedReference が返るはず: {err:?}");
    };
    assert_eq!(span, span_to_source_span(of_span), "報告される span は [of=...] 引数自身の位置のはず");
    assert_ne!(span, span_to_source_span(theorem_span), "定理環境の span が使われてはいけない");
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn validate_refs_recurses_into_list_items_and_table_cells() {
    // Arrange
    let registry = CounterRegistry::default_for_seiran();
    let list_nodes = vec![ResolvedNode::List {
      ordered: false,
      items: vec![ResolvedListItem {
        content: vec![ResolvedNode::Paragraph(vec![ResolvedInline::Ref {
          target: LabelId::new("missing"),
          span: Span::DUMMY,
        }])],
        marker: None,
        item_gap: None,
      }],
      start: None,
      item_gap: None,
    }];

    // Act
    let list_err = validate_refs(&list_nodes, &registry, Origin::Source(SourceId::new(0))).unwrap_err();

    // Assert
    assert!(matches!(list_err, ResolveError::UnresolvedReference { ref label, .. } if label == "missing"));

    // Arrange: テーブルセルの中の参照も検証対象
    let table_nodes = vec![ResolvedNode::Table {
      columns: Vec::new(),
      widths: Vec::new(),
      head: Vec::new(),
      rows: vec![ResolvedTableRow {
        cells: vec![ResolvedTableCell {
          content: vec![ResolvedInline::Ref {
            target: LabelId::new("missing"),
            span: Span::DUMMY,
          }],
          span: 1,
        }],
        rule_above: false,
      }],
      caption: None,
      caption_position: crate::model::CaptionPosition::Bottom,
      label: None,
      counter_value: crate::resolve::counter::CounterValue {
        kind: crate::resolve::counter::CounterKind::Counter(crate::config::CounterName::Table),
        parts: vec![1],
      },
      span: Span::DUMMY,
      breakable: true,
    }];

    // Act
    let table_err = validate_refs(&table_nodes, &registry, Origin::Source(SourceId::new(0))).unwrap_err();

    // Assert
    assert!(matches!(table_err, ResolveError::UnresolvedReference { ref label, .. } if label == "missing"));
  }
}
