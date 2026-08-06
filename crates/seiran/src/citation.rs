//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の意味解析・
//! CSL 整形・参考文献リスト（書誌）生成までを 1 module に閉じる。
//!
//! 「引用箇所について判明した事実」（`crate::resolve::analyze` が HIR 走査の一部として作る
//! `CitationSiteFacts`）と「そこから作る生成物」（[`generate_citations`]）の 2 段構成で、
//! 著者が書いた文書木（HIR）へは一切書き戻さない。表示は `NodeId` をキーにする
//! side table として返す（生成物自体は書誌・引用表示専用に絞られた `GeneratedBlock` / `GeneratedInline`
//! で表す、#325）。

mod bridge;
mod generate;
mod references;
mod render;
mod style;
#[cfg(test)]
pub(crate) mod test_fixtures;

pub(crate) use generate::{CitationFormatError, GeneratedCitations, generate_citations};
pub use references::{Reference, References, read_references};
pub(crate) use style::{CitationStyleError, load_citation_style};
