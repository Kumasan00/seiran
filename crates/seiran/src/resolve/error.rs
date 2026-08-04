//! 解決ステージで発生し得るエラー

use miette::Diagnostic;
use thiserror::Error;

use crate::model::Origin;

/// 解決（ラベル・`\ref`・`\cite` の名前解決）で発生し得るエラー
#[derive(Debug, Error, Diagnostic)]
#[non_exhaustive]
pub enum ResolveError {
  /// `\ref{label}` / `proof` の `[of=...]` が参照するラベルが未定義の場合
  #[error("未解決の参照です: ラベル `{label}`")]
  #[diagnostic(code(resolve::unresolved_reference), help("対応する label が定義されているか確認してください。"))]
  UnresolvedReference {
    /// 解決できなかったラベル名
    label: String,
    /// `\ref{...}` のソース位置
    #[label("この参照が未解決です")]
    span: miette::SourceSpan,
    /// この参照が属する起源
    origin: Origin,
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
    /// この重複定義が属する起源
    origin: Origin,
  },

  /// 文献引用（`\cite{...}`）が CSL 整形ステージを経ずに解決ステージへ到達した場合
  #[error("未整形の文献引用が解決ステージに到達しました: キー `{keys}`")]
  #[diagnostic(
    code(resolve::unresolved_citation),
    help("resolve の前に citation::process_citations が実行されているか確認してください。")
  )]
  UnresolvedCitation {
    /// 採番できなかった引用キー列
    keys: String,
    /// `\cite{...}` のソース位置
    #[label("この引用が未整形です")]
    span: miette::SourceSpan,
    /// この引用が属する起源
    origin: Origin,
  },
}

impl ResolveError {
  /// このエラーが帰属する起源を返す
  #[must_use]
  pub fn origin(&self) -> Origin {
    return match self {
      ResolveError::UnresolvedReference { origin, .. }
      | ResolveError::DuplicateLabel { origin, .. }
      | ResolveError::UnresolvedCitation { origin, .. } => *origin,
    };
  }
}

/// `crate::model::Span` を診断用の `miette::SourceSpan` へ変換する
///
/// `ResolveError` のバリアントはいずれも `#[label]` に `miette::SourceSpan` を要求するため、
/// カウンタ登録（`counter`）とツリー構築（`resolver`）の双方から共有する
pub(crate) fn span_to_source_span(span: crate::model::Span) -> miette::SourceSpan {
  return miette::SourceSpan::from((span.start as usize, span.len() as usize));
}
