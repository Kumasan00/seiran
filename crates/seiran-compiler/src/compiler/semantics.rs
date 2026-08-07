//! 意味解析（ラベル登録・`\ref` 検証・カウンタ構造値の確定・引用箇所の解析）と `\cite` の
//! CSL 整形を 1 回の呼び出しの背後に隠す。
//!
//! analyze → CSL 整形という呼び出し順序は、この module の外からは見えない（issue #303）。
//! 生成物（引用表示・書誌）は著者が書いた文書木へは一切書き戻さず、[`Semantics`] の別フィールドに
//! 置いたまま組版へ渡る。

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  document::HirDocument,
  semantics::{
    self, AnalyzedDocument, CitationFormatError, CitationStyleError, GeneratedCitations, References, SemanticError,
  },
};

/// 意味解析と CSL 整形の成果物
///
/// 著者が書いた内容と判明した事実（`analyzed`）と、そこから生成した表示・書誌（`generated`）を
/// 混ぜずに束ねる。
#[derive(Debug)]
pub(super) struct Semantics {
  /// HIR と意味解析の事実
  pub(super) analyzed: AnalyzedDocument,
  /// CSL 整形が生成した引用表示と書誌
  pub(super) generated: GeneratedCitations,
}

/// `resolve_semantics` のエラー。
///
/// 内側の意味解析 / CSL スタイル読込 / CSL 整形それぞれの診断（code・help・label）をそのまま運ぶ。
/// 呼び出し元（`compiler.rs`）が帰属ソースを組み立てられるよう、`resolve::SemanticError` は
/// ここでは変換せず `SourceId` だけを運ぶ形のまま渡し、`SourceDb` から本文を引く変換は
/// `compiler.rs::wrap_resolve_error` に委ねる。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum SemanticsError {
  /// CSL スタイル（`.csl`）・ロケールの読込・解析エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  CitationStyle(#[from] CitationStyleError),
  /// `\cite` の CSL 整形（表示の生成）エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  CitationFormat(#[from] CitationFormatError),
  /// ラベル・`\ref`・カウンタ・引用キーの意味解析エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Analyze(#[from] SemanticError),
}

/// HIR を意味解析し、引用の表示と書誌を生成して [`Semantics`] にまとめて返す。
///
/// `document`（HIR）からラベル・参照・カウンタ・見出し・引用箇所の事実を取り、引用箇所の事実から
/// 表示インライン列と書誌を生成する。生成物は著者が書いた文書木へは一切書き戻さない。
///
/// # Errors
///
/// 意味解析（重複ラベル・未解決参照・未定義引用キー）、CSL スタイルの読込、または表示の生成に
/// 失敗した場合にエラーを返す。
pub(super) fn resolve_semantics(
  source: &dyn crate::project::ProjectSource,
  document: HirDocument,
  references: &References,
  style: &crate::config::Style,
) -> Result<Semantics, SemanticsError> {
  // ラベル・参照・カウンタ・見出し・引用箇所の意味解析はここで完了する
  // （以降 `\cite` のキーは必ず参照定義に存在する）。意味解決には表示設定を渡さない
  // （`DocumentPolicy` は値に影響する設定だけの投影）。
  let policy = crate::config::DocumentPolicy::from_style(style);
  let analyzed = semantics::analyze(document, &policy, references)?;
  // 引用が 1 つも無ければ CSL スタイルを読まない（`csl_path` 未設定でもエラーにしない）。
  let generated = if analyzed.has_citations() {
    let compiled = semantics::load_citation_style(source, style)?;
    semantics::generate_citations(analyzed.citation_sites(), references, &compiled, &style.reference.title)?
  } else {
    GeneratedCitations::default()
  };

  return Ok(Semantics {
    analyzed,
    generated,
  });
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::{SemanticsError, resolve_semantics};
  use crate::{
    compiler::golden::{enter_workspace_root, load_base},
    config::Style,
    document::HirDocument,
    frontend::parse_source,
    project::{FilesystemProjectSource, MemoryProjectSource},
    semantics::{CitationStyleError, read_references},
    source::SourceId,
  };

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

    // Act
    let semantics =
      resolve_semantics(&source, document, &references, &style).expect("citation → resolve の連携は成功するはず");

    // Assert — 書誌と表示が生成され、意味解析の成果物と並んで返る
    assert!(!semantics.generated.is_empty(), "cite.sei は引用を含むので生成物は空でないはず");
    assert!(!semantics.generated.bibliography().is_empty(), "cite.sei は引用を含むので書誌が生成されるはず");
    assert!(semantics.analyzed.has_citations(), "cite.sei の引用箇所は事実として記録されるはず");
    for (site, _) in semantics.analyzed.citation_sites().iter() {
      assert!(!semantics.generated.display_at(site).is_empty(), "全引用箇所に表示が付くはず: {site:?}");
    }
  }

  #[test]
  fn resolve_semantics_maps_citation_error() {
    // Arrange — 既知キーの \cite を含むソースを、csl_path 未設定のまま渡す。
    // キーは既知にしておかないと analyze の未知キー検証で先に弾かれてしまうため、
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

    // Act
    let error =
      resolve_semantics(&source, document, &references, &style).expect_err("csl_path 未設定はエラーになるはず");

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

    // Act
    let error =
      resolve_semantics(&source, document, &references, &style).expect_err("未定義ラベル参照はエラーになるはず");

    // Assert
    assert!(matches!(error, SemanticsError::Analyze(_)), "got: {error:?}");
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

    // Act
    let error = resolve_semantics(&source, document, &references, &style).expect_err("未知キーはエラー");

    // Assert
    assert!(
      matches!(error, SemanticsError::Analyze(crate::semantics::SemanticError::UnknownCitationKeys { .. })),
      "got: {error:?}"
    );
  }
}
