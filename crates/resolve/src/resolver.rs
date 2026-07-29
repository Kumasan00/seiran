//! pass1: `SemanticDocument` → `ResolvedDocument` の変換 + ラベル・カウンタ登録

use model::{DocNode, InlineNode, LabelId, Origin};

use crate::{
  ResolveError,
  counter::CounterRegistry,
  error::span_to_source_span,
  inline::{IndexKey, ResolvedInline},
  node::{ResolvedListItem, ResolvedMathRow, ResolvedNode, ResolvedTableCell, ResolvedTableRow},
};

/// pass1 走査中に集める見出しの生データ（表示文字列化は typeset 側の責務）
///
/// Task 6（`resolve_project`）が見出し一覧の組み立てに使う想定の先行 API。現時点では
/// `resolve_group` がテストからのみ呼ばれ、フィールドの読み出し側がまだ無いため
/// `dead_code` を明示的に許可する
#[allow(dead_code)]
pub(crate) struct PendingHeading {
  /// 見出しの文書順インデックス
  pub(crate) index: usize,
  /// 見出しレベル
  pub(crate) level: model::HeadingLevel,
  /// カウンタ値（無採番の見出しは `None`）
  pub(crate) counter_value: Option<crate::counter::CounterValue>,
  /// 見出しタイトル（`\ref` 解決済み）
  pub(crate) title: Vec<ResolvedInline>,
  /// この見出しが属する起源
  pub(crate) source: Origin,
}

/// `nodes` を順に変換する（1 ソースグループぶん）
///
/// `typeset::lowering::lower_nodes_inner` / `lower_node_indexed` と同じ走査順を再現する。
///
/// # Errors
///
/// ラベルの重複登録・未整形の `\cite` に遭遇した場合に [`ResolveError`] を返します。
///
/// テストからは呼ばれているが、Task 6 の `resolve_project` から配線されるまで通常ビルドの
/// `dead_code` 判定はテスト側の呼び出しを数えないため、明示的に許可する
#[allow(dead_code)]
pub(crate) fn resolve_group(
  nodes: &[DocNode],
  registry: &mut CounterRegistry,
  headings: &mut Vec<PendingHeading>,
  source: Origin,
) -> Result<Vec<ResolvedNode>, ResolveError> {
  let mut result = Vec::with_capacity(nodes.len());
  for node in nodes {
    result.push(resolve_node(node, registry, headings, source)?);
  }
  return Ok(result);
}

/// 単一の `DocNode` を解決する
fn resolve_node(
  node: &DocNode,
  registry: &mut CounterRegistry,
  headings: &mut Vec<PendingHeading>,
  source: Origin,
) -> Result<ResolvedNode, ResolveError> {
  match node {
    DocNode::Heading {
      level,
      numbered,
      title,
      label,
      span,
    } => {
      let counter_value = if *numbered {
        Some(registry.increment_with_label(
          CounterRegistry::counter_name_for_heading(*level),
          label.as_deref(),
          *span,
          source,
        )?)
      } else {
        None
      };
      let resolved_label = label.as_ref().map(|l| return LabelId::new(l.clone()));
      let resolved_title = resolve_inlines(title, source)?;
      headings.push(PendingHeading {
        index: headings.len(),
        level: *level,
        counter_value: counter_value.clone(),
        title: resolved_title.clone(),
        source,
      });
      return Ok(ResolvedNode::Heading {
        level: *level,
        numbered: *numbered,
        title: resolved_title,
        label: resolved_label,
        span: *span,
      });
    },
    DocNode::Paragraph(inlines) => return Ok(ResolvedNode::Paragraph(resolve_inlines(inlines, source)?)),
    DocNode::List {
      ordered,
      items,
      start,
      item_gap,
    } => {
      let items = items
        .iter()
        .map(|item| {
          return Ok(ResolvedListItem {
            content: resolve_group(&item.content, registry, headings, source)?,
            marker: item.marker.clone(),
            item_gap: item.item_gap,
          });
        })
        .collect::<Result<Vec<_>, ResolveError>>()?;
      return Ok(ResolvedNode::List {
        ordered: *ordered,
        items,
        start: *start,
        item_gap: *item_gap,
      });
    },
    DocNode::Theorem {
      class,
      title,
      body,
      of,
      label,
      span,
    } => {
      let counter_value = registry.increment_theorem_with_label(*class, label.as_deref(), *span, source)?;
      // `of` の参照先はこの時点ではまだ登録済みとは限らない（前方参照— 定理は後で
      // 定義されうる）。存在確認は pass2（`validate_refs`）に委ね、ここでは LabelId 化のみ行う。
      let of = of.as_ref().map(|target| return LabelId::new(target.label.clone()));
      return Ok(ResolvedNode::Theorem {
        class: *class,
        title: title.clone(),
        body: resolve_group(body, registry, headings, source)?,
        of,
        label: label.as_ref().map(|l| return LabelId::new(l.clone())),
        counter_value,
        span: *span,
      });
    },
    DocNode::Quote { kind, body } => {
      return Ok(ResolvedNode::Quote {
        kind: *kind,
        body: resolve_group(body, registry, headings, source)?,
      });
    },
    DocNode::Rule { width, height } => {
      return Ok(ResolvedNode::Rule {
        width: *width,
        height: *height,
      });
    },
    DocNode::PageBreak => return Ok(ResolvedNode::PageBreak),
    DocNode::Space(length) => return Ok(ResolvedNode::Space(*length)),
    DocNode::Anchor(target) => return Ok(ResolvedNode::Anchor(target.clone())),
    DocNode::MathBlock {
      kind,
      rows,
      numbered,
      label,
      span,
    } => {
      let rows = rows
        .iter()
        .map(|row| {
          let row_value = if row.numbered {
            let row_span = row.label_span.unwrap_or(*span);
            Some(registry.increment_with_label(
              config::CounterName::Equation,
              row.label.as_deref(),
              row_span,
              source,
            )?)
          } else {
            None
          };
          return Ok(ResolvedMathRow {
            cells: row.cells.clone(),
            numbered: row.numbered,
            label: row.label.as_ref().map(|l| return LabelId::new(l.clone())),
            label_span: row.label_span,
            counter_value: row_value,
          });
        })
        .collect::<Result<Vec<_>, ResolveError>>()?;
      let env_value = if *numbered {
        Some(registry.increment_with_label(config::CounterName::Equation, label.as_deref(), *span, source)?)
      } else {
        None
      };
      return Ok(ResolvedNode::MathBlock {
        kind: *kind,
        rows,
        numbered: *numbered,
        label: label.as_ref().map(|l| return LabelId::new(l.clone())),
        counter_value: env_value,
        span: *span,
      });
    },
    DocNode::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      label,
      span,
    } => {
      let counter_value =
        registry.increment_with_label(config::CounterName::Figure, label.as_deref(), *span, source)?;
      return Ok(ResolvedNode::Figure {
        image_path: image_path.clone(),
        width: *width,
        height: *height,
        dpi: *dpi,
        downsample: *downsample,
        caption: caption.as_ref().map(|c| return resolve_inlines(c, source)).transpose()?,
        caption_position: *caption_position,
        label: label.as_ref().map(|l| return LabelId::new(l.clone())),
        counter_value,
        span: *span,
      });
    },
    DocNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      breakable,
      label,
      span,
    } => {
      let counter_value = registry.increment_with_label(config::CounterName::Table, label.as_deref(), *span, source)?;
      let resolve_rows = |table_rows: &[model::TableRow]| {
        return table_rows
          .iter()
          .map(|row| {
            return Ok(ResolvedTableRow {
              cells: row
                .cells
                .iter()
                .map(|cell| {
                  return Ok(ResolvedTableCell {
                    content: resolve_inlines(&cell.content, source)?,
                    span: cell.span,
                  });
                })
                .collect::<Result<Vec<_>, ResolveError>>()?,
              rule_above: row.rule_above,
            });
          })
          .collect::<Result<Vec<_>, ResolveError>>();
      };
      return Ok(ResolvedNode::Table {
        columns: columns.clone(),
        widths: widths.clone(),
        head: resolve_rows(head)?,
        rows: resolve_rows(rows)?,
        caption: caption.as_ref().map(|c| return resolve_inlines(c, source)).transpose()?,
        caption_position: *caption_position,
        label: label.as_ref().map(|l| return LabelId::new(l.clone())),
        counter_value,
        span: *span,
        breakable: *breakable,
      });
    },
  }
}

/// インラインノード列を解決する
fn resolve_inlines(inlines: &[InlineNode], source: Origin) -> Result<Vec<ResolvedInline>, ResolveError> {
  return inlines.iter().map(|inline| return resolve_inline(inline, source)).collect();
}

/// 単一のインラインノードを解決する
fn resolve_inline(inline: &InlineNode, source: Origin) -> Result<ResolvedInline, ResolveError> {
  return Ok(match inline {
    InlineNode::Text(s) => ResolvedInline::Text(s.clone()),
    InlineNode::Styled { kind, children } => ResolvedInline::Styled {
      kind: *kind,
      children: resolve_inlines(children, source)?,
    },
    InlineNode::Colored { color, children } => ResolvedInline::Colored {
      color: *color,
      children: resolve_inlines(children, source)?,
    },
    InlineNode::InlineMath(nodes) => ResolvedInline::InlineMath(nodes.clone()),
    InlineNode::Symbol(ch) => ResolvedInline::Symbol(*ch),
    InlineNode::LineBreak => ResolvedInline::LineBreak,
    InlineNode::NoIndent => ResolvedInline::NoIndent,
    InlineNode::Ref { label, span } => ResolvedInline::Ref {
      target: LabelId::new(label.clone()),
      span: *span,
    },
    InlineNode::Link { url, children } => ResolvedInline::Link {
      url: url.clone(),
      children: resolve_inlines(children, source)?,
    },
    InlineNode::InternalLink { target, children } => ResolvedInline::InternalLink {
      target: target.clone(),
      children: resolve_inlines(children, source)?,
    },
    InlineNode::Cite { keys, label, span } => {
      let Some(label) = label else {
        return Err(ResolveError::UnresolvedCitation {
          keys: keys.join(", "),
          span: span_to_source_span(*span),
          origin: source,
        });
      };
      ResolvedInline::Cite {
        targets: keys.iter().map(|k| return model::CitationId::new(k.clone())).collect(),
        label: resolve_inlines(label, source)?,
        span: *span,
      }
    },
    InlineNode::Footnote { body, span } => ResolvedInline::Footnote {
      body: resolve_inlines(body, source)?,
      span: *span,
    },
    InlineNode::Index {
      word,
      reading,
      span,
    } => ResolvedInline::Index {
      key: IndexKey {
        word: word.clone(),
        reading: reading.clone(),
      },
      span: *span,
    },
  });
}

#[cfg(test)]
mod tests {
  use model::{HeadingLevel, InlineNode, Origin, SourceId, Span};

  use super::*;
  use crate::counter::CounterRegistry;

  #[allow(clippy::unwrap_used)]
  #[test]
  fn resolve_group_assigns_counter_value_to_labeled_heading() {
    // Arrange
    let nodes = vec![model::DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::text("Intro")],
      label: Some("ch:intro".to_string()),
      span: Span::DUMMY,
    }];
    let mut registry = CounterRegistry::from_style(&config::Style::default());
    let mut headings = Vec::new();

    // Act
    let resolved = resolve_group(&nodes, &mut registry, &mut headings, Origin::Source(SourceId::new(0))).unwrap();

    // Assert
    let ResolvedNode::Heading { label, .. } = &resolved[0] else {
      panic!("Heading が期待されます: {resolved:?}");
    };
    assert_eq!(label.as_ref().map(model::LabelId::as_str), Some("ch:intro"));
    assert!(registry.resolve_label("ch:intro").is_some(), "ラベルは登録済みのはず");
  }

  #[allow(clippy::unwrap_used)]
  #[test]
  fn resolve_group_reports_duplicate_label() {
    // Arrange
    let heading = |label: &str| {
      return model::DocNode::Heading {
        level: HeadingLevel::Chapter,
        numbered: true,
        title: vec![InlineNode::text("T")],
        label: Some(label.to_string()),
        span: Span::DUMMY,
      };
    };
    let nodes = vec![heading("dup"), heading("dup")];
    let mut registry = CounterRegistry::from_style(&config::Style::default());
    let mut headings = Vec::new();

    // Act
    let err = resolve_group(&nodes, &mut registry, &mut headings, Origin::Source(SourceId::new(0))).unwrap_err();

    // Assert
    assert!(matches!(err, crate::ResolveError::DuplicateLabel { ref label, .. } if label == "dup"));
  }
}
