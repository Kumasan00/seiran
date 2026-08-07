//! 意味解析の入口 — HIR の走査（ラベル登録・`\ref` 検証・カウンタ構造値の確定・引用箇所の収集）と
//! `\cite` の CSL 整形を 1 回の呼び出しの背後に隠す。
//!
//! 走査 → CSL 整形という呼び出し順序は、この module の外からは見えない（issue #303 / #349）。
//! 生成物（引用表示・書誌）は著者が書いた文書木へは一切書き戻さず、[`Semantics`] の別フィールドに
//! 置いたまま組版へ渡る。

use crate::{
  document::HirDocument,
  semantics::{
    AnalyzedDocument, GeneratedCitations, References, error::AnalyzeError, generate_citations, load_citation_style,
    walk,
  },
};

/// 意味解析と CSL 整形の成果物
///
/// 著者が書いた内容と判明した事実（`analyzed`）と、そこから生成した表示・書誌（`generated`）を
/// 混ぜずに束ねる。
#[derive(Debug)]
pub(crate) struct Semantics {
  /// HIR と意味解析の事実
  pub(crate) analyzed: AnalyzedDocument,
  /// CSL 整形が生成した引用表示と書誌
  pub(crate) generated: GeneratedCitations,
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
pub(crate) fn analyze(
  source: &dyn crate::project::ProjectSource,
  document: HirDocument,
  references: &References,
  style: &crate::config::Style,
) -> Result<Semantics, AnalyzeError> {
  // ラベル・参照・カウンタ・見出し・引用箇所の走査はここで完了する
  // （以降 `\cite` のキーは必ず参照定義に存在する）。走査には表示設定を渡さない
  // （`DocumentPolicy` は値に影響する設定だけの投影）。
  let policy = crate::config::DocumentPolicy::from_style(style);
  let facts = walk::collect_facts(&document, &policy, references)?;
  let analyzed = AnalyzedDocument::new(document, facts);
  let generated = generate(source, &analyzed, references, style)?;

  return Ok(Semantics {
    analyzed,
    generated,
  });
}

/// CSL を読まずに走査だけを行うテスト専用の入口
///
/// 本体経路の [`analyze`] は `ProjectSource` と CSL スタイルを要求するが、走査結果だけを見る
/// テスト（lowering・引用整形の単体テスト）はそこを通らない。
///
/// # Errors
///
/// 重複ラベル・未解決参照・未定義引用キーがある場合にエラーを返す。
#[cfg(test)]
pub(crate) fn analyze_for_test(
  document: HirDocument,
  policy: &crate::config::DocumentPolicy,
  references: &References,
) -> Result<AnalyzedDocument, crate::semantics::SemanticError> {
  let facts = walk::collect_facts(&document, policy, references)?;
  return Ok(AnalyzedDocument::new(document, facts));
}

/// 引用箇所の事実から CSL 整形の生成物を作る。
///
/// 引用が 1 つも無ければ CSL スタイルを読まない（`csl_path` 未設定でもエラーにしない）。
fn generate(
  source: &dyn crate::project::ProjectSource,
  analyzed: &AnalyzedDocument,
  references: &References,
  style: &crate::config::Style,
) -> Result<GeneratedCitations, AnalyzeError> {
  if !analyzed.has_citations() {
    return Ok(GeneratedCitations::default());
  }
  let compiled = load_citation_style(source, style)?;
  let generated = generate_citations(analyzed.citation_sites(), references, &compiled, &style.reference.title)?;
  return Ok(generated);
}

#[cfg(test)]
mod tests {
  use super::analyze;
  use crate::{
    config::Style,
    document::HirDocument,
    frontend::parse_source,
    project::{FilesystemProjectSource, MemoryProjectSource},
    semantics::{
      AnalyzeError, CitationStyleError, read_references,
      test_fixtures::{ieee_csl_path, sample_references},
    },
    source::SourceId,
  };

  #[test]
  fn analyze_composes_walk_then_citation() {
    // Arrange — 実 CSL（tests/data/ieee.csl）と参照定義で、走査 → CSL 整形の連携を確認する
    let source = FilesystemProjectSource::new();
    let references = sample_references();
    let mut style = Style::default();
    style.reference.csl_path = Some(ieee_csl_path());
    let source_id = SourceId::new(0);
    let hir = parse_source(r"本文 \cite{kwan2014} と \cite{doe2020}", source_id).expect("パースに成功するはず");
    let document = HirDocument::assemble(vec![hir]);

    // Act
    let semantics = analyze(&source, document, &references, &style).expect("走査 → CSL 整形の連携は成功するはず");

    // Assert — 書誌と表示が生成され、意味解析の成果物と並んで返る
    assert!(!semantics.generated.is_empty(), "引用を含む入力なので生成物は空でないはず");
    assert!(!semantics.generated.bibliography().is_empty(), "引用を含む入力なので書誌が生成されるはず");
    assert!(semantics.analyzed.has_citations(), "引用箇所は事実として記録されるはず");
    for (site, _) in semantics.analyzed.citation_sites().iter() {
      assert!(!semantics.generated.display_at(site).is_empty(), "全引用箇所に表示が付くはず: {site:?}");
    }
  }

  #[test]
  fn analyze_maps_citation_error() {
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
    let error = analyze(&source, document, &references, &style).expect_err("csl_path 未設定はエラーになるはず");

    // Assert
    assert!(matches!(error, AnalyzeError::CitationStyle(CitationStyleError::MissingCslPath)), "got: {error:?}");
  }

  #[test]
  fn analyze_maps_resolve_error() {
    // Arrange — 未解決の \ref を含むソース（引用なし）を渡し、Resolve エラーへ写像されることを確認する
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let hir = parse_source(r"\ref{missing}", source_id).expect("パースは成功するはず");
    let document = HirDocument::assemble(vec![hir]);

    // Act
    let error = analyze(&source, document, &references, &style).expect_err("未定義ラベル参照はエラーになるはず");

    // Assert
    assert!(matches!(error, AnalyzeError::Analyze(_)), "got: {error:?}");
  }

  #[test]
  fn analyze_reports_unknown_citation_key() {
    // Arrange — 参照定義が空のまま `\cite` を含むソースを渡す
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let hir = parse_source(r"\cite{missing-key}", source_id).expect("パースは成功するはず");
    let document = HirDocument::assemble(vec![hir]);

    // Act
    let error = analyze(&source, document, &references, &style).expect_err("未知キーはエラー");

    // Assert
    assert!(
      matches!(error, AnalyzeError::Analyze(crate::semantics::SemanticError::UnknownCitationKeys { .. })),
      "got: {error:?}"
    );
  }
}
