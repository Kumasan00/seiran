//! `\cite` のキー存在検証（pass2）
//!
//! パーサ（pass1）が生成した [`InlineNode::Cite`] スタブを走査し、各引用キーが参照定義
//! （references）に存在するかを検証する。`\ref` の解決（[`crate::evaluator::counter::resolve_refs`]）と
//! 異なり、`label` の書き換えは行わない（最終的な引用ラベルの整形は CSL 整形ステージの責務）。
//!
//! 未定義キーは最初の 1 件で短絡せず、ファイル内のすべてを収集して
//! [`EvalError::UnknownCitationKeys`] に集約して報告する。

use std::collections::HashSet;

use document::{DocNode, InlineNode, ListItem};
use miette::LabeledSpan;

use crate::evaluator::EvalError;

/// `Vec<DocNode>` を再帰的に走査して `\cite` の引用キー存在を検証する
///
/// `keys` は参照定義（references）の有効な参照 ID の集合。未定義のキーを含む `\cite` を
/// すべて収集し、1 件でもあれば [`EvalError::UnknownCitationKeys`] に集約して返す。
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
      // 数式・図（キャプションなし）・罫線・改ページ・スペース・アンカーには `\cite` は出現しない
      // （`DocNode::Anchor` は CSL 整形ステージが parser の後に追加するため、ここには届かない）
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
      | InlineNode::InternalLink { children, .. } => {
        collect_unknown_in_inlines(children, keys, labels);
      },
      InlineNode::Cite {
        keys: cite_keys,
        span,
        ..
      } => {
        let missing: Vec<&str> =
          cite_keys.iter().filter(|key| !keys.contains(key.as_str())).map(String::as_str).collect();
        if !missing.is_empty() {
          labels.push(LabeledSpan::new_with_span(Some(format!("未定義の引用キー: {}", missing.join(", "))), *span));
        }
      },
      InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::Ref { .. } => {},
    }
  }
}

#[cfg(test)]
mod tests {
  use document::{DocNode, InlineNode};
  use miette::SourceSpan;

  use super::*;

  fn span() -> SourceSpan { return SourceSpan::from((0_usize, 5_usize)); }

  fn keys(values: &[&str]) -> HashSet<String> { return values.iter().map(|v| (*v).to_string()).collect(); }

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
    // Arrange — 2 つの段落それぞれに未定義キーを含む \cite がある
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

    // Assert — 2 件すべてが集約される
    let Err(EvalError::UnknownCitationKeys { labels }) = result else {
      panic!("UnknownCitationKeys が期待されます");
    };
    assert_eq!(labels.len(), 2);
  }
}
