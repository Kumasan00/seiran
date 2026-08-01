//! テキストソースから Document IR への変換 — 字句解析・構文解析・評価を 1 クレートに統合

use std::collections::HashSet;

use bumpalo::Bump;
use miette::Diagnostic;
use model::DocNode;
use thiserror::Error;
use tracing::debug;

mod evaluator;
mod span_ext;
mod syntax;

pub use evaluator::EvalError;
use evaluator::cite::resolve_cites;

/// `parse_source` が返すエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ParseSourceError {
  /// 構文解析（`crate::syntax::parse`）で発生したエラー
  #[error("構文解析に失敗しました")]
  #[diagnostic(code(frontend::parse_source::syntax))]
  Syntax {
    /// このエラーが属するソースの識別子（本文は呼び出し元の `SourceDb` が保持する）
    source_id: model::SourceId,
    /// 元の構文エラー
    #[source]
    #[diagnostic_source]
    error: crate::syntax::ParserError,
  },

  /// 評価（CST → Document IR 変換）で発生したエラー
  #[error("評価に失敗しました")]
  #[diagnostic(code(frontend::parse_source::eval))]
  Eval {
    /// このエラーが属するソースの識別子（本文は呼び出し元の `SourceDb` が保持する）
    source_id: model::SourceId,
    /// 元の評価エラー
    #[source]
    #[diagnostic_source]
    error: EvalError,
  },
}

impl ParseSourceError {
  /// このエラーが属するソースの識別子を返す
  #[must_use]
  pub fn source_id(&self) -> model::SourceId {
    return match self {
      ParseSourceError::Syntax { source_id, .. } | ParseSourceError::Eval { source_id, .. } => *source_id,
    };
  }
}

/// ソーステキストをパースして Document IR（`Vec<DocNode>`）を生成する
///
/// # Errors
///
/// パースまたは評価で失敗した場合に [`ParseSourceError`] を返します。
// `citation_keys` は呼び出し側が既定ハッシャで構築した集合をそのまま受けるため、
// BuildHasher を総称化せず `HashSet<String>` で受ける（implicit_hasher を許可）。
#[allow(clippy::implicit_hasher)]
pub fn parse_source(
  source: &str,
  source_id: model::SourceId,
  citation_keys: &HashSet<String>,
) -> Result<Vec<DocNode>, ParseSourceError> {
  let arena = Bump::new();
  let cst = crate::syntax::parse(source, &arena, evaluator::lookup_env_parse_mode).map_err(|error| {
    return ParseSourceError::Syntax { source_id, error };
  })?;

  let doc_nodes = evaluator::evaluate_children(source, cst).map_err(|error| {
    return ParseSourceError::Eval { source_id, error };
  })?;

  resolve_cites(&doc_nodes, citation_keys).map_err(|error| {
    return ParseSourceError::Eval { source_id, error };
  })?;

  debug!(source_id = source_id.index(), node_count = doc_nodes.len(), "ソースのパース・評価が完了しました");
  return Ok(doc_nodes);
}
