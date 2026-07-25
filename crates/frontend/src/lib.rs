//! テキストソースから Document IR への変換 — 字句解析・構文解析・評価を 1 クレートに統合

use std::collections::HashSet;

use bumpalo::Bump;
use miette::{Diagnostic, NamedSource};
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
    /// ソース名付きの元テキスト（`#[label]` をレンダリングするための `source_code`）
    #[source_code]
    src: NamedSource<String>,
    /// 元の構文エラー
    #[source]
    #[diagnostic_source]
    error: crate::syntax::ParserError,
  },

  /// 評価（CST → Document IR 変換）で発生したエラー
  #[error("評価に失敗しました")]
  #[diagnostic(code(frontend::parse_source::eval))]
  Eval {
    /// ソース名付きの元テキスト（`#[label]` をレンダリングするための `source_code`）
    #[source_code]
    src: NamedSource<String>,
    /// 元の評価エラー
    #[source]
    #[diagnostic_source]
    error: EvalError,
  },
}

/// ソーステキストをパースして Document IR（`Vec<DocNode>`）を生成する
///
/// # Errors
///
/// パースまたは評価で失敗した場合に [`ParseSourceError`] を返します。
// `ParseSourceError` は `EvalError` のフィールドが大きく ~168 バイトになるが、
// `parse_source` はソースファイルごとに 1 回しか呼ばれないため Result のサイズは
// 性能上の問題にならない。Box<dyn Diagnostic + Send + Sync> で型消去すると呼び出し側で
// 内側エラーの variant match ができなくなるため、具体型のまま返してこの lint を抑止する。
// `citation_keys` は呼び出し側が既定ハッシャで構築した集合をそのまま受けるため、
// BuildHasher を総称化せず `HashSet<String>` で受ける（implicit_hasher を許可）。
#[allow(clippy::result_large_err, clippy::implicit_hasher)]
pub fn parse_source(
  source: &str,
  source_name: &str,
  citation_keys: &HashSet<String>,
) -> Result<Vec<DocNode>, ParseSourceError> {
  let arena = Bump::new();
  let cst = crate::syntax::parse(source, &arena, evaluator::lookup_env_parse_mode).map_err(|error| {
    return ParseSourceError::Syntax {
      src: NamedSource::new(source_name, source.to_string()),
      error,
    };
  })?;

  let doc_nodes = evaluator::evaluate_children(source, cst).map_err(|error| {
    return ParseSourceError::Eval {
      src: NamedSource::new(source_name, source.to_string()),
      error,
    };
  })?;

  resolve_cites(&doc_nodes, citation_keys).map_err(|error| {
    return ParseSourceError::Eval {
      src: NamedSource::new(source_name, source.to_string()),
      error,
    };
  })?;

  debug!(source_path = source_name, node_count = doc_nodes.len(), "ソースのパース・評価が完了しました");
  return Ok(doc_nodes);
}
