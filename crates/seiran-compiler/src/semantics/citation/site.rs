//! 引用キー [`CitationId`] と、引用箇所について判明した事実 [`CitationSiteFacts`]。

/// `\cite{key}` の引用キー
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CitationId(String);

impl CitationId {
  /// 新しい `CitationId` を生成する
  #[must_use]
  pub(crate) fn new(key: impl Into<String>) -> Self { return CitationId(key.into()); }

  /// 内部の文字列を返す
  #[must_use]
  pub(crate) fn as_str(&self) -> &str { return &self.0; }
}

/// 1 つの引用箇所（`\cite{...}`）について判明した事実
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CitationSiteFacts {
  /// 引用先（`\cite{a,b}` はソース上の順序で 2 件）
  pub(crate) targets: Vec<CitationId>,
}
