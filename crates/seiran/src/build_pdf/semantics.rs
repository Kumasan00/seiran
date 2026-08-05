//! `\cite` の CSL 整形と意味解決（ラベル登録・`\ref` 検証・カウンタ構造値の確定・
//! typed ID 割当）を 1 回の呼び出しの背後に隠す。
//!
//! citation → resolve という呼び出し順序と、書誌を `resolve::SemanticDocument::bibliography`
//! として実ソースの `groups` とは別枠で渡す組み立ては、この module の外からは見えない
//! （issue #303）。

use miette::Diagnostic;
use thiserror::Error;

use super::ParsedSource;
use crate::{
  citation::{self, CitationError, References},
  model::{DocNode, SourceId},
  resolve::{self, ResolveError, ResolvedDocument, SemanticDocument, SemanticGroup},
};

/// `resolve_semantics` のエラー。
///
/// 内側の citation / resolve それぞれの診断（code・help・label）をそのまま運ぶ。呼び出し元
/// （`build_pdf.rs`）が `Resolve` について帰属ソースを組み立てられるよう、`resolve::ResolveError`
/// はここでは変換せずそのまま保持する。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum SemanticsError {
  /// `\cite` の CSL 整形エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Citation(#[from] CitationError),
  /// ラベル・`\ref`・カウンタの解決エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Resolve(#[from] ResolveError),
}

/// 全ソースの `\cite` を CSL 整形し、その結果を意味解決へ渡して `ResolvedDocument` を返す。
///
/// `parsed` は所有権ごと受け取り、citation による書き換えは内部で保持する値の上でのみ行う
/// （呼び出し元が別に保持している AST を破壊的に変更することはない）。
///
/// # Errors
///
/// CSL 整形または意味解決に失敗した場合にエラーを返す。
pub(super) fn resolve_semantics(
  source: &dyn crate::config::ProjectSource,
  parsed: Vec<ParsedSource>,
  references: &References,
  style: &crate::config::Style,
) -> Result<ResolvedDocument, SemanticsError> {
  let source_ids: Vec<SourceId> = parsed.iter().map(|p| return p.source_id).collect();
  let docs: Vec<Vec<DocNode>> = parsed.into_iter().map(|p| return p.nodes).collect();

  let (docs, bibliography) = citation::process_citations(docs, references, style, source)?;

  let groups: Vec<SemanticGroup<'_>> = source_ids
    .into_iter()
    .zip(docs.iter())
    .map(|(source_id, nodes)| {
      return SemanticGroup {
        nodes: nodes.as_slice(),
        source_id,
      };
    })
    .collect();
  let semantic = SemanticDocument {
    groups,
    bibliography: bibliography.as_slice(),
  };

  let resolved = resolve::resolve_project(&semantic, style)?;
  return Ok(resolved);
}

#[cfg(test)]
mod tests {
  use std::{collections::HashSet, fs};

  use super::{ParsedSource, SemanticsError, resolve_semantics};
  use crate::{
    build_pdf::golden::{enter_workspace_root, load_base},
    citation::{CitationError, read_references},
    config::{FilesystemProjectSource, MemoryProjectSource, Style},
    frontend::parse_source,
    model::{DocNode, InlineNode, NodeId, SourceId, Span},
  };

  #[test]
  fn resolve_semantics_composes_citation_then_resolve() {
    // Arrange — 実 fixture（cite.sei + 文献 + CSL）で citation → resolve の連携を確認する
    enter_workspace_root();
    let (_config, style, references) = load_base();
    let source = FilesystemProjectSource::new();
    let content = fs::read_to_string("tests/text/cite.sei").expect("fixture cite.sei を読めるはず");
    let source_id = SourceId::new(0);
    let citation_keys: HashSet<String> = references.keys().cloned().collect();
    let hir = parse_source(&content, source_id, &citation_keys).expect("fixture cite.sei のパースに成功するはず");
    let document = crate::model::HirDocument::assemble(vec![hir]);
    let group = document.groups().first().expect("1 ソース分のグループがあるはず");
    let parsed = vec![ParsedSource {
      source_id,
      nodes: crate::frontend::hir_group_to_doc_nodes(group, document.locations()),
    }];

    // Act
    let resolved =
      resolve_semantics(&source, parsed, &references, &style).expect("citation → resolve の連携は成功するはず");

    // Assert — 書誌が生成され resolve 済みドキュメントへ渡っている
    assert!(!resolved.bibliography.is_empty(), "cite.sei は引用を含むので書誌が生成されるはず");
  }

  #[test]
  fn resolve_semantics_maps_citation_error() {
    // Arrange — csl_path 未設定のまま \cite を含むソースを渡し、Citation エラーへ写像されることを確認する
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let nodes = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["missing-key".to_string()],
      node_id: NodeId::for_test(SourceId::new(0), 0),
      label: None,
      span: Span::DUMMY,
    }])];
    let parsed = vec![ParsedSource { source_id, nodes }];

    // Act
    let error = resolve_semantics(&source, parsed, &references, &style).expect_err("csl_path 未設定はエラーになるはず");

    // Assert
    assert!(matches!(error, SemanticsError::Citation(CitationError::MissingCslPath)), "got: {error:?}");
  }

  #[test]
  fn resolve_semantics_maps_resolve_error() {
    // Arrange — 未解決の \ref を含むソース（引用なし）を渡し、Resolve エラーへ写像されることを確認する
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let nodes = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: Span::DUMMY,
    }])];
    let parsed = vec![ParsedSource { source_id, nodes }];

    // Act
    let error =
      resolve_semantics(&source, parsed, &references, &style).expect_err("未定義ラベル参照はエラーになるはず");

    // Assert
    assert!(matches!(error, SemanticsError::Resolve(_)), "got: {error:?}");
  }
}
