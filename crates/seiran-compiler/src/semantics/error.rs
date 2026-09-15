//! 意味解析で発生し得るエラー
//!
//! 入口 [`analyze`](fn@crate::semantics::analyze) が返す [`AnalyzeError`] と、HIR 走査が返す
//! [`SemanticFailures`] / [`SemanticError`] の 2 層に分かれる。後者は必ずソース位置に帰属する
//! （`source_id` を持つ）ため、呼び出し元は本文を添えた診断へ組み替えられる。CSL の読込・整形
//! エラーはソース位置を持たないので、この不変条件を壊さないよう [`SemanticError`] には混ぜず
//! [`AnalyzeError`] の別バリアントに置く。

use std::collections::HashMap;

use miette::{Diagnostic, LabeledSpan};
use thiserror::Error;

use crate::{
  document::{NodeId, SourceLocation},
  failures::Failures,
  semantics::{CitationFormatError, CitationStyleError},
  source::{SourceId, Span},
};

/// [`analyze`](fn@crate::semantics::analyze) のエラー
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

/// 表示単位（1 診断 = 1 ソース）に分けた意味解析エラーの非空集合。
///
/// 未定義引用キーだけは 1 回の走査で複数ソースに跨って見つかるが、miette は 1 診断につき
/// `source_code` を 1 つしか持てないため、表示のためにソースごとへ分ける。分けるのは
/// **semantics 自身**で、診断文・`code`・help を `compiler` 側へ複製しない。
pub(crate) type SemanticFailures = Failures<SemanticError>;

/// 文書順の未定義引用箇所を、初出ソース順にソースごとの診断へまとめる。
///
/// 同じソース内の複数箇所は 1 診断のラベルとして並べる（箇所ごとに独立した修正ではなく
/// 「このソースの `\cite` キーが参照定義と合っていない」という 1 問題として読めるため）。
/// miette は 1 診断につき `source_code` を 1 つしか持てないので、ソースを跨いで束ねることはできない。
///
/// 各診断には、他の種別の診断と文書順にマージするための位置としてそのソースの**最初の**引用箇所を
/// 添えて返す。1 箇所も無ければ空を返す。
pub(crate) fn group_unknown_citations(sites: &[UnknownCitationSite]) -> Vec<(NodeId, SemanticError)> {
  // 出現順を保つため、初出順の Vec に積んでから組み立てる。
  let mut order: Vec<(SourceId, NodeId)> = Vec::new();
  let mut per_source: HashMap<SourceId, Vec<LabeledSpan>> = HashMap::new();
  for site in sites {
    let labels = per_source.entry(site.source_id).or_insert_with(|| {
      order.push((site.source_id, site.site));
      return Vec::new();
    });
    labels.push(LabeledSpan::new_with_span(
      Some(format!("未定義の引用キー: {}", site.keys.join(", "))),
      site.span.to_source_span(),
    ));
  }
  return order
    .into_iter()
    .map(|(source_id, first_site)| {
      let Some(labels) = per_source.remove(&source_id) else {
        unreachable!("order には per_source へ登録した SourceId しか入らない")
      };
      return (first_site, SemanticError::UnknownCitationKeys { source_id, labels });
    })
    .collect();
}

/// 未定義キーを含む引用箇所 1 件
#[derive(Debug, Clone)]
pub(super) struct UnknownCitationSite {
  /// `\cite{...}` のノード（他種別の診断と文書順にマージするための位置）
  pub site: NodeId,
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
pub(crate) enum SemanticError {
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
  ///
  /// 構築は [`SemanticError::duplicate_label`] だけ。2 回目の定義を主ラベルに、最初の定義が同じソースに
  /// あればそれを 2 本目のラベルに並べる。別ソースにあるときは 1 診断が `source_code` を 1 つしか
  /// 持てないので、[`SemanticError::first_definition_elsewhere`] が返す関連診断で示す（#552）。
  #[error("ラベルが重複しています: {label}")]
  #[diagnostic(code(semantics::duplicate_label), help("label=... の値はドキュメント全体で一意にしてください"))]
  DuplicateLabel {
    /// 重複したラベル名
    label: String,
    /// 2 回目の定義（主ラベル）と、同じソースにあれば最初の定義の位置
    #[label(collection)]
    labels: Vec<LabeledSpan>,
    /// この重複定義（2 回目）が属するソース
    source_id: SourceId,
    /// 最初の定義の位置（先勝ちで有効なまま残っている定義）
    first_definition: SourceLocation,
  },
}

impl SemanticError {
  /// このエラーが帰属するソースを返す
  ///
  /// 全バリアントが必ず 1 つのソースに帰属する（引用キーのエラーもソースごとに分割済み）。
  /// `analyze` は実ソースしか走査しないので、帰属先が実ソース以外になることはない。
  #[must_use]
  pub(crate) fn source_id(&self) -> SourceId {
    return match self {
      SemanticError::UnknownCitationKeys { source_id, .. }
      | SemanticError::UnresolvedReference { source_id, .. }
      | SemanticError::DuplicateLabel { source_id, .. } => *source_id,
    };
  }

  /// ラベル `label` の重複定義 `duplicate` を、最初の定義 `first` とともに報告する診断を作る。
  ///
  /// 最初の定義が同じソースにあれば同じスニペットの 2 本目のラベルとして示す。別ソースにあれば
  /// ラベルは 2 回目の 1 本だけで、最初の定義は [`SemanticError::first_definition_elsewhere`] が
  /// 返す関連診断になる（本文を添えるのは compiler）。
  pub(crate) fn duplicate_label(label: &str, duplicate: SourceLocation, first: SourceLocation) -> Self {
    let mut labels = vec![LabeledSpan::new_primary_with_span(
      Some("このラベルは既に定義されています".to_string()),
      duplicate.span.to_source_span(),
    )];
    if first.source_id == duplicate.source_id {
      labels.push(LabeledSpan::new_with_span(Some("最初の定義はここです".to_string()), first.span.to_source_span()));
    }
    return SemanticError::DuplicateLabel {
      label: label.to_string(),
      labels,
      source_id: duplicate.source_id,
      first_definition: first,
    };
  }

  /// この診断と同じ 1 つの問題を、主診断とは別のソースで示す関連診断を返す（無ければ `None`）。
  ///
  /// 関連診断は `SourceId` と span だけを持ち本文を持たない。本文を添えて主診断の `related` へ連結するのは
  /// compiler の `SourceDiagnostic`（1 診断が持てる `source_code` は 1 つなので、主診断の本文では描けない）。
  #[must_use]
  pub(crate) fn first_definition_elsewhere(&self) -> Option<FirstLabelDefinition> {
    return match self {
      SemanticError::DuplicateLabel {
        label,
        source_id,
        first_definition,
        ..
      } if first_definition.source_id != *source_id => Some(FirstLabelDefinition {
        label: label.clone(),
        span: first_definition.span.to_source_span(),
        source_id: first_definition.source_id,
      }),
      SemanticError::DuplicateLabel { .. }
      | SemanticError::UnknownCitationKeys { .. }
      | SemanticError::UnresolvedReference { .. } => None,
    };
  }
}

/// 重複ラベルの最初の定義が主診断と別のソースにあるときの、その位置を示す関連診断
///
/// 独立した修正箇所ではなく主診断と同じ 1 つの問題の別の位置なので、`code` は持たず severity は
/// `Advice`（#376 の `related` の用途）。本文は持たず、compiler の `SourceDiagnostic` が
/// `source_id` のソース本文を添える。
#[derive(Debug, Error, Diagnostic)]
#[error("ラベル `{label}` の最初の定義")]
#[diagnostic(severity(Advice))]
pub(crate) struct FirstLabelDefinition {
  /// 重複したラベル名
  label: String,
  /// 最初に定義したコマンド / 環境のソース位置
  #[label("最初の定義はここです")]
  span: miette::SourceSpan,
  /// 最初の定義が属するソース
  source_id: SourceId,
}

impl FirstLabelDefinition {
  /// 最初の定義が属するソースを返す。
  #[must_use]
  pub(crate) fn source_id(&self) -> SourceId { return self.source_id; }
}
