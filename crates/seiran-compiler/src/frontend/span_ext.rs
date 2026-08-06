//! `crate::source::Span` から `miette::SourceSpan` への変換ヘルパー
//!
//! 両者はいずれも frontend にとって外部型（`crate::source` / `miette`）のため、orphan rule により
//! `From` では変換を書けない。パーサ・評価器の診断構築点はこのヘルパー経由で変換する。

use crate::source::Span;

/// `Span` を `miette::SourceSpan` に変換する
pub(crate) trait ToSourceSpan {
  /// `miette::SourceSpan` に変換する
  fn to_source_span(self) -> miette::SourceSpan;
}

impl ToSourceSpan for Span {
  fn to_source_span(self) -> miette::SourceSpan {
    return miette::SourceSpan::new(miette::SourceOffset::from(self.start as usize), self.len() as usize);
  }
}
