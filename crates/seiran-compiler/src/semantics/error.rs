//! 意味解析で発生し得るエラー
//!
//! 入口 [`analyze`](crate::semantics::analyze) が返す [`AnalyzeError`] と、HIR 走査が返す
//! [`SemanticError`] の 2 層に分かれる。後者は必ずソース位置に帰属する（`source_id` を持つ）ため、
//! 呼び出し元は本文を添えた診断へ組み替えられる。CSL の読込・整形エラーはソース位置を持たないので、
//! この不変条件を壊さないよう [`SemanticError`] には混ぜず [`AnalyzeError`] の別バリアントに置く。

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  semantics::{CitationFormatError, CitationStyleError},
  source::{SourceId, Span},
};

/// [`analyze`](crate::semantics::analyze) のエラー
///
/// 内側の意味解析 / CSL スタイル読込 / CSL 整形それぞれの診断（code・help・label）をそのまま運ぶ。
/// 呼び出し元（`compiler`）が帰属ソースを組み立てられるよう、[`SemanticError`] はここでは変換せず
/// `SourceId` だけを運ぶ形のまま渡し、`SourceSet` から本文を引く変換は
/// `compiler.rs::wrap_resolve_error` に委ねる。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum AnalyzeError {
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

/// 未定義キーを含む引用箇所 1 件
#[derive(Debug, Clone)]
pub struct UnknownCitationSite {
  /// この引用箇所が属するソース
  pub source_id: SourceId,
  /// `\cite{...}` のソース位置
  pub span: Span,
  /// 参照定義に見つからなかったキー
  pub keys: Vec<String>,
}

/// 解決（ラベル登録・`\ref` の名前解決・引用キーの存在検証）で発生し得るエラー
#[derive(Debug, Error, Diagnostic)]
#[non_exhaustive]
pub enum SemanticError {
  /// `\cite{...}` のキーが参照定義に存在しない場合（全箇所を集約して 1 度に報告する）
  #[error("未定義の引用キーがあります")]
  #[diagnostic(
    code(citation::semantic::unknown_citation_key),
    help("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください")
  )]
  UnknownCitationKeys {
    /// 未定義キーを含む引用箇所（文書順）
    sites: Vec<UnknownCitationSite>,
  },

  /// `\ref{label}` / `proof` の `[of=...]` が参照するラベルが未定義の場合
  #[error("未解決の参照です: ラベル `{label}`")]
  #[diagnostic(code(resolve::unresolved_reference), help("対応する label が定義されているか確認してください。"))]
  UnresolvedReference {
    /// 解決できなかったラベル名
    label: String,
    /// `\ref{...}` のソース位置
    #[label("この参照が未解決です")]
    span: miette::SourceSpan,
    /// この参照が属するソース
    source_id: SourceId,
  },

  /// `label=...` で同名ラベルが重複登録された場合
  #[error("ラベルが重複しています: {label}")]
  #[diagnostic(code(resolve::duplicate_label), help("label=... の値はドキュメント全体で一意にしてください"))]
  DuplicateLabel {
    /// 重複したラベル名
    label: String,
    /// 2 回目に定義したコマンド / 環境のソース位置
    #[label("このラベルは既に定義されています")]
    span: miette::SourceSpan,
    /// この重複定義が属するソース
    source_id: SourceId,
  },
}

impl SemanticError {
  /// このエラーが帰属するソースを返す
  ///
  /// 引用キーのエラーは箇所ごとに `SourceId` を持つ（1 診断が複数ソースに跨りうる）ため `None`。
  /// `analyze` は実ソースしか走査しないので、生成物（書誌）由来のエラーはそもそも存在しない。
  #[must_use]
  pub fn source_id(&self) -> Option<SourceId> {
    return match self {
      SemanticError::UnresolvedReference { source_id, .. } | SemanticError::DuplicateLabel { source_id, .. } => {
        Some(*source_id)
      },
      SemanticError::UnknownCitationKeys { .. } => None,
    };
  }
}

/// `crate::source::Span` を診断用の `miette::SourceSpan` へ変換する
///
/// `SemanticError` のバリアントはいずれも `#[label]` に `miette::SourceSpan` を要求するため、
/// カウンタ登録（`counter`）とツリー構築（`resolver`）の双方から共有する
pub(crate) fn span_to_source_span(span: crate::source::Span) -> miette::SourceSpan {
  return miette::SourceSpan::from((span.start as usize, span.len() as usize));
}
