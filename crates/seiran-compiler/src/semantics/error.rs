//! 意味解析で発生し得るエラー
//!
//! 入口 [`analyze`](crate::semantics::analyze) が返す [`AnalyzeError`] と、HIR 走査が返す
//! [`SemanticFailures`] / [`SemanticError`] の 2 層に分かれる。後者は必ずソース位置に帰属する
//! （`source_id` を持つ）ため、呼び出し元は本文を添えた診断へ組み替えられる。CSL の読込・整形
//! エラーはソース位置を持たないので、この不変条件を壊さないよう [`SemanticError`] には混ぜず
//! [`AnalyzeError`] の別バリアントに置く。

use miette::{Diagnostic, LabeledSpan};
use thiserror::Error;

use crate::{
  semantics::{CitationFormatError, CitationStyleError},
  source::{SourceId, Span},
};

/// [`analyze`](crate::semantics::analyze) のエラー
///
/// 内側の意味解析 / CSL スタイル読込 / CSL 整形のいずれかを `?` で運ぶための制御フロー型で、
/// **表示単位ではない**（`miette::Diagnostic` を実装しない）。呼び出し元（`compiler`）が
/// 必ず全バリアントを分解し、内側の leaf 診断だけがユーザーへ届く。
#[derive(Debug, Error)]
pub(crate) enum AnalyzeError {
  /// CSL スタイル（`.csl`）・ロケールの読込・解析エラー
  #[error(transparent)]
  CitationStyle(#[from] CitationStyleError),
  /// `\cite` の CSL 整形（表示の生成）エラー
  #[error(transparent)]
  CitationFormat(#[from] CitationFormatError),
  /// ラベル・`\ref`・カウンタ・引用キーの意味解析エラー
  #[error(transparent)]
  Analyze(#[from] SemanticFailures),
}

/// 表示単位（1 診断 = 1 ソース）に分けた意味解析エラーの集合。
///
/// 未定義引用キーだけは 1 回の走査で複数ソースに跨って見つかるが、miette は 1 診断につき
/// `source_code` を 1 つしか持てないため、表示のためにソースごとへ分ける。分けるのは
/// **semantics 自身**で、診断文・`code`・help を `compiler` 側へ複製しない。
///
/// `first` が主診断。空では構築できない（構築経路は [`SemanticFailures::single`] と
/// [`SemanticFailures::from_unknown_citations`] だけで、後者は空なら `None` を返す）。
#[derive(Debug)]
pub(crate) struct SemanticFailures {
  /// 主診断
  first: SemanticError,
  /// 主診断と同時に報告する残りの診断（初出ソース順）
  rest: Vec<SemanticError>,
}

impl SemanticFailures {
  /// 単一のエラーから集合を作る。
  pub(crate) fn single(error: SemanticError) -> Self {
    return SemanticFailures {
      first: error,
      rest: Vec::new(),
    };
  }

  /// 文書順の未定義引用箇所を、初出ソース順にソースごとの診断へまとめる。
  ///
  /// 同じソース内の複数箇所は 1 診断のラベルとして並べる。1 箇所も無ければ `None`。
  pub(crate) fn from_unknown_citations(sites: Vec<UnknownCitationSite>) -> Option<Self> {
    // 出現順を保つため、初出順の Vec に積んでから組み立てる。
    let mut order: Vec<SourceId> = Vec::new();
    let mut per_source: std::collections::HashMap<SourceId, Vec<LabeledSpan>> = std::collections::HashMap::new();
    for site in sites {
      let labels = per_source.entry(site.source_id).or_insert_with(|| {
        order.push(site.source_id);
        return Vec::new();
      });
      labels.push(LabeledSpan::new_with_span(
        Some(format!("未定義の引用キー: {}", site.keys.join(", "))),
        span_to_source_span(site.span),
      ));
    }

    let mut errors = order.into_iter().map(|source_id| {
      let Some(labels) = per_source.remove(&source_id) else {
        unreachable!("order には per_source へ登録した SourceId しか入らない")
      };
      return SemanticError::UnknownCitationKeys { source_id, labels };
    });
    let first = errors.next()?;
    return Some(SemanticFailures {
      first,
      rest: errors.collect(),
    });
  }

  /// 主診断と残りの診断へ分解する。
  pub(crate) fn into_parts(self) -> (SemanticError, Vec<SemanticError>) { return (self.first, self.rest); }

  /// 主診断を返す。
  #[cfg(test)]
  pub(crate) fn first(&self) -> &SemanticError { return &self.first; }

  /// 主診断から順にすべての診断を返す。
  #[cfg(test)]
  pub(crate) fn iter(&self) -> impl Iterator<Item = &SemanticError> {
    return std::iter::once(&self.first).chain(self.rest.iter());
  }
}

impl From<SemanticError> for SemanticFailures {
  fn from(error: SemanticError) -> Self { return SemanticFailures::single(error); }
}

impl std::fmt::Display for SemanticFailures {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return self.first.fmt(f); }
}

impl std::error::Error for SemanticFailures {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return std::error::Error::source(&self.first); }
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
  /// `\cite{...}` のキーが参照定義に存在しない場合（1 ソース分をまとめて 1 度に報告する）
  ///
  /// 同じソース内の複数箇所はラベルを並べる（箇所ごとに未定義キーが違うため、
  /// `#[label(collection)]` に静的な文言は付けない）。複数ソースに跨る場合は
  /// [`SemanticFailures`] がソースごとのこの診断を並べる。
  #[error("未定義の引用キーがあります")]
  #[diagnostic(
    code(semantics::unknown_citation_key),
    help("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください")
  )]
  UnknownCitationKeys {
    /// この診断が属するソース
    source_id: SourceId,
    /// このソース内の `\cite{...}` ごとの未定義キー（文書順）
    #[label(collection)]
    labels: Vec<LabeledSpan>,
  },

  /// `\ref{label}` / `proof` の `[of=...]` が参照するラベルが未定義の場合
  #[error("未解決の参照です: ラベル `{label}`")]
  #[diagnostic(code(semantics::unresolved_reference), help("対応する label が定義されているか確認してください。"))]
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
  #[diagnostic(code(semantics::duplicate_label), help("label=... の値はドキュメント全体で一意にしてください"))]
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
  /// 全バリアントが必ず 1 つのソースに帰属する（引用キーのエラーもソースごとに分割済み）。
  /// `analyze` は実ソースしか走査しないので、帰属先が実ソース以外になることはない。
  #[must_use]
  pub fn source_id(&self) -> SourceId {
    return match self {
      SemanticError::UnknownCitationKeys { source_id, .. }
      | SemanticError::UnresolvedReference { source_id, .. }
      | SemanticError::DuplicateLabel { source_id, .. } => *source_id,
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
