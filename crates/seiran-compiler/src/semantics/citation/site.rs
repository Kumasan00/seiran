//! 引用キー [`CitationId`] と、引用箇所について判明した事実 [`CitationSiteFacts`]。
//!
//! [`CitationSiteFacts`] は [`super::generate_citations`] の入力契約なので、後段である
//! `citation` が所有し、前段の走査 `semantics::walk::collect_facts` が構築する（後段が要求する入力契約は
//! 後段が所有する）。[`CitationId`] は引用キーの型付き表現で、事実・生成物・書誌アンカーの
//! いずれもが同じ引用の同一性を指すため、同じ module に置く。

/// `\cite{key}` の引用キー
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CitationId(String);

impl CitationId {
  /// 新しい `CitationId` を生成する
  #[must_use]
  pub fn new(key: impl Into<String>) -> Self { return CitationId(key.into()); }

  /// 内部の文字列を返す
  #[must_use]
  pub fn as_str(&self) -> &str { return &self.0; }
}

/// 1 つの引用箇所（`\cite{...}`）について判明した事実
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CitationSiteFacts {
  /// 引用先（`\cite{a,b}` はソース上の順序で 2 件）
  pub(crate) targets: Vec<CitationId>,
}
