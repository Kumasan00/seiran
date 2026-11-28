//! 意味解析で発生し得るエラー
//!
//! 入口 [`analyze`](fn@crate::semantics::analyze) が返す [`AnalyzeError`] と、HIR 走査が返す
//! [`SemanticFailures`] / [`SemanticError`] の 2 層に分かれる。後者は必ずソース位置に帰属する
//! （`source_id` を持つ）。CSL の読込エラーはソース位置を持たないので、この不変条件を壊さないよう
//! [`SemanticError`] には混ぜず [`AnalyzeError`] の別バリアントに置く。

use std::collections::BTreeMap;

use itertools::Itertools;
use miette::{Diagnostic, LabeledSpan};
use thiserror::Error;

use crate::{
  document::{HeadingLevel, NodeId, SourceLocation},
  failures::Failures,
  semantics::ReadCitationStyleError,
  source::{SourceId, Span},
};

/// [`analyze`](fn@crate::semantics::analyze) のエラー
///
/// 内側の意味解析 / CSL スタイル読込のいずれかを `?` で運ぶための制御フロー型で、
/// **表示単位ではない**（`miette::Diagnostic` を実装しない）。
#[derive(Debug, Error)]
pub(crate) enum AnalyzeError {
  /// CSL スタイル（`.csl`）・ロケールの読込・解析エラー
  #[error(transparent)]
  CitationStyle(#[from] ReadCitationStyleError),
  /// ラベル・`\ref`・カウンタ・引用キーの意味解析エラー
  #[error(transparent)]
  Semantic(#[from] SemanticFailures),
}

/// 表示単位（1 診断 = 1 ソース）に分けた意味解析エラーの非空集合。
///
/// 未定義引用キーだけは 1 回の走査で複数ソースに跨って見つかるが、miette は 1 診断につき
/// `source_code` を 1 つしか持てないため、ソースごとへ分ける。
pub(super) type SemanticFailures = Failures<SemanticError>;

/// 文書順の未定義引用箇所を、ソース順（`SourceId` 昇順 = 宣言順）にソースごとの診断へまとめる。
///
/// 同じソース内の複数箇所は 1 診断のラベルとして並べる。そのソースの未定義キーに `,` を含むものが 1 つでもあれば、help に `\,` の案内を足す。
///
/// 各診断には、他の種別の診断と文書順にマージするための位置としてそのソースの**最初の**引用箇所を
/// 添えて返す。1 箇所も無ければ空を返す。
pub(super) fn group_unknown_citations(sites: &[UnknownCitationSite]) -> Vec<(NodeId, SemanticError)> {
  // ソースごとの（最初の引用箇所, ラベル列, 未定義キーに `,` を含むものがあったか）。
  // `BTreeMap` の反復順が `SourceId` の順序（宣言順）そのもの。
  let mut per_source: BTreeMap<SourceId, (NodeId, Vec<LabeledSpan>, bool)> = BTreeMap::new();
  for site in sites {
    let (_, labels, has_comma_key) =
      per_source.entry(site.source_id).or_insert_with(|| return (site.site, Vec::new(), false));
    labels.push(LabeledSpan::new_with_span(Some(unknown_keys_label(&site.keys)), site.span));
    *has_comma_key |= site.keys.iter().any(|key| return key.contains(','));
  }
  return per_source
    .into_iter()
    .map(|(source_id, (first_site, labels, has_comma_key))| {
      let comma_hint = if has_comma_key {
        ESCAPED_COMMA_HINT
      } else {
        ""
      };
      return (
        first_site,
        SemanticError::UnknownCitationKeys {
          source_id,
          labels,
          comma_hint,
        },
      );
    })
    .collect();
}

/// 未定義キーに `,` を含むとき help の末尾に足す案内
///
/// `,` を含むキーは `\,` で書いたもの（エスケープしない `,` はキーの区切りなのでキーに残らない）。
/// 先頭の句点は基本文との継ぎ目。
const ESCAPED_COMMA_HINT: &str =
  "。`\\,` はキーの中の文字 `,` です。キーを区切るにはエスケープしない `,` を使ってください";

/// 引用箇所 1 件の未定義キーを並べたラベル文字列を組み立てる。
///
/// キーは `\,` で書いた `,` を含みうるので、区切りの `, ` だけでは `,` を含む 1 キーと
/// 複数キーの境目が読めない。各キーをバッククォートで括り、境目を字面から一意にする。
fn unknown_keys_label(keys: &[String]) -> String {
  return format!("未定義の引用キー: {}", keys.iter().map(|key| return format!("`{key}`")).join(", "));
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

/// 解決（ラベル登録・`\ref` の名前解決・引用キーの存在検証・複数参照と範囲参照の妥当性検証）で発生し得るエラー
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum SemanticError {
  /// `\cite{...}` のキーが参照定義に存在しない場合（1 ソース分をまとめて 1 度に報告する）
  #[error("未定義の引用キーがあります")]
  #[diagnostic(
    code(semantics::unknown_citation_key),
    help("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください{comma_hint}")
  )]
  UnknownCitationKeys {
    /// この診断が属するソース
    source_id: SourceId,
    /// このソース内の `\cite{...}` ごとの未定義キー（文書順）
    #[label(collection)]
    labels: Vec<LabeledSpan>,
    /// help の末尾に足す `\,` の案内（このソースの未定義キーに `,` を含むものが無ければ空文字列）
    comma_hint: &'static str,
  },

  /// `\ref{label}` / `\hyperref{label}{...}` / `proof` の `[of=...]` が参照するラベルが未定義の場合
  #[error("未解決の参照です: ラベル `{label}`")]
  #[diagnostic(code(semantics::unresolved_ref), help("対応する label が定義されているか確認してください。"))]
  UnresolvedRef {
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
  /// 構築は [`SemanticError::duplicate_label`] だけ。最初の定義が別ソースにあるときは 1 診断が
  /// `source_code` を 1 つしか持てないので、[`SemanticError::related_elsewhere`] が返す
  /// 関連診断で示す。
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

  /// `\refs{...}` の参照先に種類（カウンタ名、または定理クラス）の違う項目が混ざっている場合
  #[error("1 つの \\refs の参照先の種類が揃っていません")]
  #[diagnostic(
    code(semantics::mixed_ref_kinds),
    help(
      "\\refs に並べられるのは同じ種類（同じ見出しレベル・図・表・数式、または同じ定理クラス）の参照先だけです。種類ごとに \\refs / \\ref を分けて書いてください"
    )
  )]
  MixedRefKinds {
    /// 先頭のラベル
    first: String,
    /// 先頭と種類の違う最初のラベル
    other: String,
    /// `\refs{...}` のソース位置
    #[label("`{first}` と `{other}` は種類が違います")]
    span: miette::SourceSpan,
    /// この参照が属するソース
    source_id: SourceId,
  },

  /// `\refrange{...}{...}` の両端が、同じ種類で番号が途切れずに続く区間の始点と終点になっていない場合
  #[error("範囲参照が不正です: {reason}")]
  #[diagnostic(
    code(semantics::invalid_ref_range),
    help(
      "\\refrange の 2 つのラベルには、同じ種類で番号が途切れずに続く項目の始点と終点を、始点を先にして指定してください"
    )
  )]
  InvalidRefRange {
    /// 範囲が成り立たない理由（両端のラベル名を含む）
    reason: String,
    /// `\refrange{...}{...}` のソース位置
    #[label("この範囲は成り立ちません")]
    span: miette::SourceSpan,
    /// この参照が属するソース
    source_id: SourceId,
  },

  /// 文書順で最初の `\appendix` より後ろに `\chapter` / `\part` がある場合
  ///
  /// 付録の下では章番号の位置に付録番号が入るので、付録の後ろの章は番号の先頭が章番号か付録番号かが一意でなくなる。
  /// 構築は [`SemanticError::heading_after_appendix`] だけ。
  #[error("付録の後ろに \\{level} は置けません")]
  #[diagnostic(
    code(semantics::heading_after_appendix),
    help(
      "付録（\\appendix）は本文の部・章をすべて書いた後ろに置いてください。付録の中の区分は \\section 以下で書きます"
    )
  )]
  HeadingAfterAppendix {
    /// 付録の後ろに置かれた見出しのレベル（`Part` か `Chapter`）
    level: HeadingLevel,
    /// 違反した見出し（主ラベル）と、同じソースにあれば最初の付録の位置
    #[label(collection)]
    labels: Vec<LabeledSpan>,
    /// 違反した見出しが属するソース
    source_id: SourceId,
    /// 文書順で最初の `\appendix` の位置
    first_appendix: SourceLocation,
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
      | SemanticError::UnresolvedRef { source_id, .. }
      | SemanticError::DuplicateLabel { source_id, .. }
      | SemanticError::MixedRefKinds { source_id, .. }
      | SemanticError::InvalidRefRange { source_id, .. }
      | SemanticError::HeadingAfterAppendix { source_id, .. } => *source_id,
    };
  }

  /// ラベル `label` の重複定義 `duplicate` を、最初の定義 `first` とともに報告する診断を作る。
  pub(super) fn duplicate_label(label: &str, duplicate: SourceLocation, first: SourceLocation) -> Self {
    let mut labels = vec![LabeledSpan::new_primary_with_span(
      Some("このラベルは既に定義されています".to_string()),
      duplicate.span,
    )];
    if first.source_id == duplicate.source_id {
      labels.push(LabeledSpan::new_with_span(Some("最初の定義はここです".to_string()), first.span));
    }
    return SemanticError::DuplicateLabel {
      label: label.to_string(),
      labels,
      source_id: duplicate.source_id,
      first_definition: first,
    };
  }

  /// 最初の付録 `first_appendix` より後ろの見出し `at` を報告する診断を作る。
  pub(super) fn heading_after_appendix(
    level: HeadingLevel,
    at: SourceLocation,
    first_appendix: SourceLocation,
  ) -> Self {
    let mut labels = vec![LabeledSpan::new_primary_with_span(
      Some("この見出しは最初の付録より後ろにあります".to_string()),
      at.span,
    )];
    if first_appendix.source_id == at.source_id {
      labels.push(LabeledSpan::new_with_span(Some("最初の付録はここです".to_string()), first_appendix.span));
    }
    return SemanticError::HeadingAfterAppendix {
      level,
      labels,
      source_id: at.source_id,
      first_appendix,
    };
  }

  /// この診断と同じ 1 つの問題を、主診断とは別のソースで示す関連診断を返す（無ければ `None`）。
  #[must_use]
  pub(crate) fn related_elsewhere(&self) -> Option<RelatedElsewhere> {
    return match self {
      SemanticError::DuplicateLabel {
        label,
        source_id,
        first_definition,
        ..
      } if first_definition.source_id != *source_id => Some(RelatedElsewhere {
        title: format!("ラベル `{label}` の最初の定義"),
        note: "最初の定義はここです",
        span: first_definition.span.into(),
        source_id: first_definition.source_id,
      }),
      SemanticError::HeadingAfterAppendix {
        source_id,
        first_appendix,
        ..
      } if first_appendix.source_id != *source_id => Some(RelatedElsewhere {
        title: "最初の付録".to_string(),
        note: "最初の付録はここです",
        span: first_appendix.span.into(),
        source_id: first_appendix.source_id,
      }),
      SemanticError::DuplicateLabel { .. }
      | SemanticError::HeadingAfterAppendix { .. }
      | SemanticError::UnknownCitationKeys { .. }
      | SemanticError::UnresolvedRef { .. }
      | SemanticError::MixedRefKinds { .. }
      | SemanticError::InvalidRefRange { .. } => None,
    };
  }
}

/// 主診断と別のソースにある関連位置（重複ラベルの最初の定義・最初の付録）を示す関連診断
///
/// 独立した修正箇所ではなく主診断と同じ 1 つの問題の別の位置なので、`code` は持たず severity は
/// `Advice`。本文は持たない。
#[derive(Debug, Error, Diagnostic)]
#[error("{title}")]
#[diagnostic(severity(Advice))]
pub(crate) struct RelatedElsewhere {
  /// 関連診断の見出し
  title: String,
  /// 関連位置に付けるラベルの文言
  note: &'static str,
  /// 関連位置（最初の定義・最初の付録のコマンド / 環境）
  #[label("{note}")]
  span: miette::SourceSpan,
  /// 関連位置が属するソース
  source_id: SourceId,
}

impl RelatedElsewhere {
  /// 関連位置が属するソースを返す。
  #[must_use]
  pub(crate) fn source_id(&self) -> SourceId { return self.source_id; }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn unknown_keys_label_separates_keys_containing_comma() {
    let keys = ["kwan2014,doe2020".to_string(), "x".to_string()];

    assert_eq!(unknown_keys_label(&keys), "未定義の引用キー: `kwan2014,doe2020`, `x`");
  }

  /// テスト用の未定義引用箇所 1 件（span は位置合わせだけに使うので local から機械的に作る）
  fn site(source: usize, local: u32, key: &str) -> UnknownCitationSite {
    let source_id = SourceId::new(source);
    return UnknownCitationSite {
      site: NodeId::for_test(source_id, local),
      source_id,
      span: Span::new(local, local + 1),
      keys: vec![key.to_string()],
    };
  }

  #[test]
  fn group_unknown_citations_attaches_first_site_of_each_source() {
    let sites = [site(0, 3, "a"), site(0, 7, "b"), site(1, 5, "c")];
    let grouped = group_unknown_citations(&sites);
    let summary: Vec<(NodeId, SourceId, usize)> = grouped
      .iter()
      .map(|(first_site, error)| {
        let SemanticError::UnknownCitationKeys {
          source_id, labels, ..
        } = error
        else {
          panic!("UnknownCitationKeys が期待されます: {error:?}");
        };
        return (*first_site, *source_id, labels.len());
      })
      .collect();
    assert_eq!(
      summary,
      vec![
        (NodeId::for_test(SourceId::new(0), 3), SourceId::new(0), 2),
        (NodeId::for_test(SourceId::new(1), 5), SourceId::new(1), 1),
      ]
    );
  }
}
