//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の意味解析・
//! CSL 整形・参考文献リスト（書誌）生成までを 1 module に閉じる。
//!
//! 「引用箇所について判明した事実」（`crate::semantics::analyze` が HIR 走査の一部として作る
//! [`CitationSiteFacts`]）と「そこから作る生成物」（[`generate_citations`]）の 2 段構成で、
//! 著者が書いた文書木（HIR）へは一切書き戻さない。引用の同一性（[`CitationId`]）・入力契約
//! （[`CitationSiteFacts`]）・生成物の語彙（[`GeneratedBlock`] / [`GeneratedInline`]）はいずれも
//! この module が所有する（#333）。生成物の collection と「全引用箇所の表示が生成済み」という
//! 完全性は [`GeneratedCitations`] が隠し、利用側は `NodeId` で表示を引く query だけを見る。

mod bridge;
mod generate;
mod generated;
mod references;
mod render;
mod site;
mod style;
#[cfg(test)]
pub(crate) mod test_fixtures;

pub(crate) use generate::{CitationFormatError, GeneratedCitations, generate_citations};
pub(crate) use generated::{GeneratedBlock, GeneratedInline, generated_inlines_to_plain_text};
pub use references::{ReadReferencesError, Reference, References, read_references};
pub(crate) use site::{CitationId, CitationSiteFacts};
pub(crate) use style::{CitationStyleError, load_citation_style};
