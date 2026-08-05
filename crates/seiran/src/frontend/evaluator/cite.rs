//! `\cite` のキー存在検証（pass2）
//!
//! 未定義キーをすべて収集し、[`EvalError::UnknownCitationKeys`] として報告する。

use std::collections::HashSet;

use miette::LabeledSpan;

use crate::{
  frontend::evaluator::EvalError,
  model::{HirInline, HirInlineKind, HirListItem, HirNode, HirNodeKind, SourceSpans},
};

/// HIR ノード列を再帰的に走査して `\cite` の引用キー存在を検証する
///
/// 診断位置は HIR ノードが持たないため `spans` から引く。
///
/// # Errors
///
/// 未定義の引用キーが 1 件以上見つかった場合に [`EvalError::UnknownCitationKeys`] を返します。
// 引用箇所を `NodeId` のまま返して frontend の境界で診断へ変換する形は #323（引用の
// vertical slice）で導入する。ここでは既存の診断（`UnknownCitationKeys`）の形を変えない。
pub(crate) fn resolve_cites(nodes: &[HirNode], spans: &SourceSpans, keys: &HashSet<String>) -> Result<(), EvalError> {
  let mut labels: Vec<LabeledSpan> = Vec::new();
  collect_unknown_in_nodes(nodes, spans, keys, &mut labels);
  if labels.is_empty() {
    return Ok(());
  }
  return Err(EvalError::UnknownCitationKeys { labels });
}

/// ブロックノード列を走査して未定義キーのラベルを `labels` に集める
fn collect_unknown_in_nodes(
  nodes: &[HirNode],
  spans: &SourceSpans,
  keys: &HashSet<String>,
  labels: &mut Vec<LabeledSpan>,
) {
  for node in nodes {
    match &node.kind {
      HirNodeKind::Heading { title: inlines, .. }
      | HirNodeKind::Paragraph(inlines)
      | HirNodeKind::Figure {
        caption: Some(inlines),
        ..
      } => collect_unknown_in_inlines(inlines, spans, keys, labels),
      HirNodeKind::List { items, .. } => {
        for item in items {
          collect_unknown_in_list_item(item, spans, keys, labels);
        }
      },
      HirNodeKind::Theorem { body, .. } | HirNodeKind::Quote { body, .. } => {
        collect_unknown_in_nodes(body, spans, keys, labels);
      },
      HirNodeKind::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter().chain(rows.iter()) {
          for cell in &row.cells {
            collect_unknown_in_inlines(&cell.content, spans, keys, labels);
          }
        }
        if let Some(inlines) = caption {
          collect_unknown_in_inlines(inlines, spans, keys, labels);
        }
      },
      HirNodeKind::MathBlock { .. }
      | HirNodeKind::Figure { caption: None, .. }
      | HirNodeKind::Rule { .. }
      | HirNodeKind::PageBreak
      | HirNodeKind::Space(_) => {},
    }
  }
}

/// リストアイテムの内容を再帰的に走査する
fn collect_unknown_in_list_item(
  item: &HirListItem,
  spans: &SourceSpans,
  keys: &HashSet<String>,
  labels: &mut Vec<LabeledSpan>,
) {
  collect_unknown_in_nodes(&item.content, spans, keys, labels);
}

/// インラインノード列を走査して未定義キーのラベルを `labels` に集める
fn collect_unknown_in_inlines(
  inlines: &[HirInline],
  spans: &SourceSpans,
  keys: &HashSet<String>,
  labels: &mut Vec<LabeledSpan>,
) {
  for inline in inlines {
    match &inline.kind {
      HirInlineKind::Styled { children, .. }
      | HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. }
      | HirInlineKind::Footnote { body: children, .. } => {
        collect_unknown_in_inlines(children, spans, keys, labels);
      },
      HirInlineKind::Cite { keys: cite_keys } => {
        let missing: Vec<&str> =
          cite_keys.iter().filter(|key| return !keys.contains(key.as_str())).map(String::as_str).collect();
        if !missing.is_empty() {
          let span = spans.span_of(inline.id);
          let source_span = miette::SourceSpan::from((span.start as usize, span.len() as usize));
          labels
            .push(LabeledSpan::new_with_span(Some(format!("未定義の引用キー: {}", missing.join(", "))), source_span));
        }
      },
      HirInlineKind::Text(_)
      | HirInlineKind::InlineMath(_)
      | HirInlineKind::Symbol(_)
      | HirInlineKind::LineBreak
      | HirInlineKind::NoIndent
      | HirInlineKind::Ref { .. }
      | HirInlineKind::Index { .. } => {},
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use crate::{
    frontend::{ParseSourceError, parse_source},
    model::SourceId,
  };

  /// 引用キーの集合を組み立てるテストヘルパ
  fn keys(values: &[&str]) -> HashSet<String> { return values.iter().map(|v| return (*v).to_string()).collect(); }

  /// ソースをパースして `\cite` のキー検証まで通すテストヘルパ
  ///
  /// HIR の `NodeId` は `HirBuilder` だけが発行するので、手組みのノードではなく実際の
  /// パース結果を検証面にする。
  fn check_cites(source: &str, citation_keys: &HashSet<String>) -> Result<(), EvalError> {
    return match parse_source(source, SourceId::new(0), citation_keys) {
      Ok(_) => Ok(()),
      Err(ParseSourceError::Eval { error, .. }) => Err(error),
      Err(other) => panic!("構文エラーは期待していない: {other:?}"),
    };
  }

  #[test]
  fn resolve_cites_accepts_known_keys() {
    // Arrange
    let source = r"本文 \cite{a,b} です。";

    // Act / Assert
    assert!(check_cites(source, &keys(&["a", "b", "c"])).is_ok());
  }

  #[test]
  fn resolve_cites_reports_unknown_key() {
    // Arrange
    let source = r"本文 \cite{a,missing} です。";

    // Act
    let result = check_cites(source, &keys(&["a"]));

    // Assert
    let Err(EvalError::UnknownCitationKeys { labels }) = result else {
      panic!("UnknownCitationKeys が期待されます");
    };
    assert_eq!(labels.len(), 1);
    // 位置はハードコードせずソース文字列から導く
    assert_eq!(labels[0].offset(), source.find(r"\cite").unwrap());
  }

  #[test]
  fn resolve_cites_aggregates_multiple_unknown_sites() {
    // Arrange
    let source = "一つ目 \\cite{x}。\n\n二つ目 \\cite{y}。";

    // Act
    let result = check_cites(source, &keys(&["a"]));

    // Assert
    let Err(EvalError::UnknownCitationKeys { labels }) = result else {
      panic!("UnknownCitationKeys が期待されます");
    };
    assert_eq!(labels.len(), 2);
  }

  #[test]
  fn resolve_cites_walks_nested_structures() {
    // Arrange — 定理環境・リスト・脚注の中の `\cite` も検証対象になる
    let source = r"\begin{itemize}\item{脚注\footnote{\cite{deep}}}\end{itemize}";

    // Act
    let result = check_cites(source, &keys(&["other"]));

    // Assert
    let Err(EvalError::UnknownCitationKeys { labels }) = result else {
      panic!("ネストした引用でも UnknownCitationKeys が期待されます");
    };
    assert_eq!(labels.len(), 1);
  }
}
