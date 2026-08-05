//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の意味解析・
//! CSL 整形・参考文献リスト（書誌）生成までを 1 module に閉じる。
//!
//! 「引用箇所について判明した事実」（[`analyze_citations`]）と「そこから作る生成物」
//! （[`generate_citations`]）の 2 段構成で、著者が書いた文書木（HIR / `DocNode`）へは
//! 一切書き戻さない。表示は `NodeId` をキーにする side table として返す。

mod analyze;
mod bridge;
mod generate;
mod references;
mod render;
mod style;
#[cfg(test)]
mod test_fixtures;

pub(crate) use analyze::{CitationSemanticError, analyze_citations};
pub(crate) use generate::{CitationFormatError, GeneratedCitations, generate_citations};
// 旧 citation crate の公開 API 保持のための re-export。`Date`/`DateCirca`/`DatePart`/`DateSeason`/
// `Name`/`NumberOrString`/`ReadReferencesError`/`ReferenceType` は crate::citation の外からまだ
// 消費されていない（`Reference`/`References`/`read_references` のみ build_pdf 側から使われる）。
#[allow(unused_imports)]
pub use references::{
  Date, DateCirca, DatePart, DateSeason, Name, NumberOrString, ReadReferencesError, Reference, ReferenceType,
  References, read_references,
};
pub(crate) use style::{CitationStyleError, load_citation_style};
