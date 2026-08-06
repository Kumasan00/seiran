//! 引用箇所について判明した事実 [`CitationSiteFacts`]。
//!
//! 意味解析（`crate::resolve::analyze`）が作り、CSL 整形（`crate::citation::generate_citations`）が
//! 消費する。両者の共有型なので、どちらにも属さない `model` に置く（`citation` に置くと
//! `resolve` → `citation` → `resolve` の循環になる）。

use crate::model::CitationId;

/// 1 つの引用箇所（`\cite{...}`）について判明した事実
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CitationSiteFacts {
  /// 引用先（`\cite{a,b}` はソース上の順序で 2 件）
  pub(crate) targets: Vec<CitationId>,
}
