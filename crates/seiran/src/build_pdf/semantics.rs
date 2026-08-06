//! `\cite` の CSL 整形と意味解決（ラベル登録・`\ref` 検証・カウンタ構造値の確定・
//! typed ID 割当）を 1 回の呼び出しの背後に隠す。
//!
//! citation → resolve という呼び出し順序と、生成物（引用表示・書誌）を
//! `resolve::SemanticDocument::generated` として実ソースの `groups` とは別枠で渡す組み立ては、
//! この module の外からは見えない（issue #303）。

use miette::Diagnostic;
use thiserror::Error;

use super::ParsedSource;
use crate::{
  citation::{self, CitationFormatError, CitationSemanticError, CitationStyleError, References},
  model::{DocNode, HirDocument, SourceId},
  resolve::{self, ResolveError, ResolvedDocument, SemanticDocument, SemanticGenerated, SemanticGroup},
};

/// `resolve_semantics` のエラー。
///
/// 内側の citation 意味解析 / CSL 整形 / resolve それぞれの診断（code・help・label）をそのまま運ぶ。
/// 呼び出し元（`build_pdf.rs`）が `Resolve` について帰属ソースを組み立てられるよう、
/// `resolve::ResolveError` はここでは変換せずそのまま保持する。`CitationSemantic`（未定義引用キー）
/// も同様に `SourceId` だけを運ぶ形のまま呼び出し元へ渡し、`SourceDb` から本文を引く変換は
/// `build_pdf.rs::wrap_citation_semantic_error` に委ねる。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum SemanticsError {
  /// `\cite` のキーが参照定義に存在しない場合（意味解析段。CSL 整形より前に検出する）
  #[error(transparent)]
  #[diagnostic(transparent)]
  CitationSemantic(#[from] CitationSemanticError),
  /// CSL スタイル（`.csl`）・ロケールの読込・解析エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  CitationStyle(#[from] CitationStyleError),
  /// `\cite` の CSL 整形（表示の生成）エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  CitationFormat(#[from] CitationFormatError),
  /// ラベル・`\ref`・カウンタの解決エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Resolve(#[from] ResolveError),
}

/// 引用箇所の意味解析・表示と書誌の生成を行い、その結果を意味解決へ渡して
/// `ResolvedDocument` を返す。
///
/// `document`（HIR）から引用箇所の事実を取り、そこから表示インライン列と書誌を生成する。
/// 生成物は `parsed`（`DocNode` 経路）へは一切書き戻さず、`SemanticDocument::generated` として
/// 別枠で resolve に渡す。
///
/// # Errors
///
/// 引用キーの存在検証、CSL スタイルの読込、表示の生成、または意味解決に失敗した場合に
/// エラーを返す。
pub(super) fn resolve_semantics(
  source: &dyn crate::config::ProjectSource,
  document: &HirDocument,
  parsed: Vec<ParsedSource>,
  references: &References,
  style: &crate::config::Style,
) -> Result<ResolvedDocument, SemanticsError> {
  // 引用キーの存在検証はここで完了する（以降 `\cite` のキーは必ず参照定義に存在する）。
  let facts = citation::analyze_citations(document, references)?;
  // 引用が 1 つも無ければ CSL スタイルを読まない（`csl_path` 未設定でもエラーにしない）。
  let generated = if facts.is_empty() {
    citation::GeneratedCitations::default()
  } else {
    let compiled = citation::load_citation_style(source, style)?;
    citation::generate_citations(&facts, references, &compiled, &style.reference.title)?
  };

  let source_ids: Vec<SourceId> = parsed.iter().map(|p| return p.source_id).collect();
  let docs: Vec<Vec<DocNode>> = parsed.into_iter().map(|p| return p.nodes).collect();

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
    generated: SemanticGenerated {
      citation_displays: generated.displays(),
      bibliography: generated.bibliography(),
    },
  };

  let resolved = resolve::resolve_project(&semantic, style)?;
  return Ok(resolved);
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::{ParsedSource, SemanticsError, resolve_semantics};
  use crate::{
    build_pdf::golden::{enter_workspace_root, load_base},
    citation::{CitationStyleError, read_references},
    config::{FilesystemProjectSource, MemoryProjectSource, Style},
    frontend::parse_source,
    model::{HirDocument, SourceId},
  };

  /// `HirDocument` の全グループを adapter 経由で `ParsedSource` へ変換するテストヘルパ
  ///
  /// `parse_project` が本体コードで行っている変換と同じもので、テストが手で
  /// `hir_group_to_doc_nodes` を呼ぶ重複を避ける。
  fn parsed_sources(document: &HirDocument) -> Vec<ParsedSource> {
    return document
      .groups()
      .iter()
      .map(|group| {
        return ParsedSource {
          source_id: group.source_id,
          nodes: crate::frontend::hir_group_to_doc_nodes(group, document.locations()),
        };
      })
      .collect();
  }

  #[test]
  fn resolve_semantics_composes_citation_then_resolve() {
    // Arrange — 実 fixture（cite.sei + 文献 + CSL）で citation → resolve の連携を確認する
    enter_workspace_root();
    let (_config, style, references) = load_base();
    let source = FilesystemProjectSource::new();
    let content = fs::read_to_string("tests/text/cite.sei").expect("fixture cite.sei を読めるはず");
    let source_id = SourceId::new(0);
    let hir = parse_source(&content, source_id).expect("fixture cite.sei のパースに成功するはず");
    let document = HirDocument::assemble(vec![hir]);
    let parsed = parsed_sources(&document);

    // Act
    let resolved = resolve_semantics(&source, &document, parsed, &references, &style)
      .expect("citation → resolve の連携は成功するはず");

    // Assert — 書誌が生成され resolve 済みドキュメントへ渡っている
    assert!(!resolved.generated.bibliography.is_empty(), "cite.sei は引用を含むので書誌が生成されるはず");
    assert!(!resolved.generated.citation_displays.is_empty(), "cite.sei の引用箇所には表示が生成されるはず");
  }

  #[test]
  fn resolve_semantics_maps_citation_error() {
    // Arrange — 既知キーの \cite を含むソースを、csl_path 未設定のまま渡す。
    // キーは既知にしておかないと analyze_citations の未知キー検証で先に弾かれてしまうため、
    // ここで確認したい CitationStyleError::MissingCslPath（load_citation_style 側）まで到達しない。
    let source = MemoryProjectSource::new().with_text(
      "/project/references.toml",
      "[ref1]\n\
       type = \"book\"\n\
       title = \"Sample\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n",
    );
    let style = Style::default();
    let references = read_references(&source, Some("/project/references.toml")).expect("参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let hir = parse_source(r"\cite{ref1}", source_id).expect("パースは成功するはず");
    let document = HirDocument::assemble(vec![hir]);
    let parsed = parsed_sources(&document);

    // Act
    let error = resolve_semantics(&source, &document, parsed, &references, &style)
      .expect_err("csl_path 未設定はエラーになるはず");

    // Assert
    assert!(matches!(error, SemanticsError::CitationStyle(CitationStyleError::MissingCslPath)), "got: {error:?}");
  }

  #[test]
  fn resolve_semantics_maps_resolve_error() {
    // Arrange — 未解決の \ref を含むソース（引用なし）を渡し、Resolve エラーへ写像されることを確認する
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let hir = parse_source(r"\ref{missing}", source_id).expect("パースは成功するはず");
    let document = HirDocument::assemble(vec![hir]);
    let parsed = parsed_sources(&document);

    // Act
    let error = resolve_semantics(&source, &document, parsed, &references, &style)
      .expect_err("未定義ラベル参照はエラーになるはず");

    // Assert
    assert!(matches!(error, SemanticsError::Resolve(_)), "got: {error:?}");
  }

  #[test]
  fn resolve_semantics_reports_unknown_citation_key() {
    // Arrange — 参照定義が空のまま `\cite` を含むソースを渡す
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let hir = parse_source(r"\cite{missing-key}", source_id).expect("パースは成功するはず");
    let document = HirDocument::assemble(vec![hir]);
    let parsed = parsed_sources(&document);

    // Act
    let error = resolve_semantics(&source, &document, parsed, &references, &style).expect_err("未知キーはエラー");

    // Assert
    assert!(matches!(error, SemanticsError::CitationSemantic(_)), "got: {error:?}");
  }
}
