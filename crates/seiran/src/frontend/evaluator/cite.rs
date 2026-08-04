//! `\cite` のキー存在検証（pass2）
//!
//! 未定義キーをすべて収集し、[`EvalError::UnknownCitationKeys`] として報告する。

use std::collections::HashSet;

use miette::LabeledSpan;

use crate::{
  frontend::evaluator::EvalError,
  model::{DocNode, InlineNode, ListItem},
};

/// `Vec<DocNode>` を再帰的に走査して `\cite` の引用キー存在を検証する
///
/// # Errors
///
/// 未定義の引用キーが 1 件以上見つかった場合に [`EvalError::UnknownCitationKeys`] を返します。
pub(crate) fn resolve_cites(nodes: &[DocNode], keys: &HashSet<String>) -> Result<(), EvalError> {
  let mut labels: Vec<LabeledSpan> = Vec::new();
  collect_unknown_in_nodes(nodes, keys, &mut labels);
  if labels.is_empty() {
    return Ok(());
  }
  return Err(EvalError::UnknownCitationKeys { labels });
}

/// ブロックノード列を走査して未定義キーのラベルを `labels` に集める
fn collect_unknown_in_nodes(nodes: &[DocNode], keys: &HashSet<String>, labels: &mut Vec<LabeledSpan>) {
  for node in nodes {
    match node {
      DocNode::Heading { title: inlines, .. }
      | DocNode::Paragraph(inlines)
      | DocNode::Figure {
        caption: Some(inlines),
        ..
      } => collect_unknown_in_inlines(inlines, keys, labels),
      DocNode::List { items, .. } => {
        for item in items {
          collect_unknown_in_list_item(item, keys, labels);
        }
      },
      DocNode::Theorem { body, .. } | DocNode::Quote { body, .. } => collect_unknown_in_nodes(body, keys, labels),
      DocNode::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter().chain(rows.iter()) {
          for cell in &row.cells {
            collect_unknown_in_inlines(&cell.content, keys, labels);
          }
        }
        if let Some(inlines) = caption {
          collect_unknown_in_inlines(inlines, keys, labels);
        }
      },
      // `Anchor` は CST 評価より後の段階で追加される。
      DocNode::MathBlock { .. }
      | DocNode::Figure { caption: None, .. }
      | DocNode::Rule { .. }
      | DocNode::PageBreak
      | DocNode::Space(_)
      | DocNode::Anchor(_) => {},
    }
  }
}

/// リストアイテムの内容を再帰的に走査する
fn collect_unknown_in_list_item(item: &ListItem, keys: &HashSet<String>, labels: &mut Vec<LabeledSpan>) {
  collect_unknown_in_nodes(&item.content, keys, labels);
}

/// インラインノード列を走査して未定義キーのラベルを `labels` に集める
fn collect_unknown_in_inlines(inlines: &[InlineNode], keys: &HashSet<String>, labels: &mut Vec<LabeledSpan>) {
  for inline in inlines {
    match inline {
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. }
      | InlineNode::InternalLink { children, .. }
      | InlineNode::Footnote { body: children, .. } => {
        collect_unknown_in_inlines(children, keys, labels);
      },
      InlineNode::Cite {
        keys: cite_keys,
        span,
        ..
      } => {
        let missing: Vec<&str> =
          cite_keys.iter().filter(|key| return !keys.contains(key.as_str())).map(String::as_str).collect();
        if !missing.is_empty() {
          let source_span = miette::SourceSpan::from((span.start as usize, span.len() as usize));
          labels
            .push(LabeledSpan::new_with_span(Some(format!("未定義の引用キー: {}", missing.join(", "))), source_span));
        }
      },
      InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::NoIndent
      | InlineNode::Ref { .. }
      | InlineNode::Index { .. } => {},
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{DocNode, InlineNode, Span};

  fn span() -> Span { return Span::new(0, 5); }

  fn keys(values: &[&str]) -> HashSet<String> { return values.iter().map(|v| return (*v).to_string()).collect(); }

  #[test]
  fn resolve_cites_accepts_known_keys() {
    // Arrange
    let nodes = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["a".to_string(), "b".to_string()],
      label: None,
      span: span(),
    }])];

    // Act / Assert
    assert!(resolve_cites(&nodes, &keys(&["a", "b", "c"])).is_ok());
  }

  #[test]
  fn resolve_cites_reports_unknown_key() {
    // Arrange
    let nodes = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["a".to_string(), "missing".to_string()],
      label: None,
      span: span(),
    }])];

    // Act
    let result = resolve_cites(&nodes, &keys(&["a"]));

    // Assert
    let Err(EvalError::UnknownCitationKeys { labels }) = result else {
      panic!("UnknownCitationKeys が期待されます");
    };
    assert_eq!(labels.len(), 1);
  }

  #[test]
  fn resolve_cites_aggregates_multiple_unknown_sites() {
    // Arrange
    let nodes = vec![
      DocNode::Paragraph(vec![InlineNode::Cite {
        keys: vec!["x".to_string()],
        label: None,
        span: span(),
      }]),
      DocNode::Paragraph(vec![InlineNode::Cite {
        keys: vec!["y".to_string()],
        label: None,
        span: span(),
      }]),
    ];

    // Act
    let result = resolve_cites(&nodes, &keys(&["a"]));

    // Assert
    let Err(EvalError::UnknownCitationKeys { labels }) = result else {
      panic!("UnknownCitationKeys が期待されます");
    };
    assert_eq!(labels.len(), 2);
  }
}
