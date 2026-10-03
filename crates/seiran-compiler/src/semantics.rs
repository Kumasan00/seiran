//! 意味解析（[`fn@analyze`]）— 著者が書いた HIR から「判明した事実」と引用の生成物を確定する段。
//!
//! 文書木は読み取り専用で書き戻さない。[`fn@analyze`] の後に初めて成立する意味上の識別子
//! （[`LabelId`] / [`HeadingKey`]）も本 module が所有する。

mod analyze;
mod citation;
mod counter;
mod error;
mod fact_collection;
mod facts;
mod ids;
mod policy;
mod semantic_document;

pub(crate) use analyze::analyze;
#[cfg(test)]
pub(crate) use analyze::test_support::analyze_for_test;
#[cfg(test)]
pub(crate) use citation::test_support;
pub(crate) use citation::{
  BibliographyEntry, CitationId, CitationSiteFacts, GeneratedCitations, GeneratedInline, ReadCitationStyleError,
  ReadReferencesError, References, generate_citations, generated_inlines_to_plain_text, load_citation_style,
  load_references,
};
#[cfg(test)]
pub(crate) use counter::CounterPart;
pub(crate) use counter::{CounterKind, CounterValue};
pub(crate) use error::{AnalyzeError, SemanticError, SemanticFailures};
pub(crate) use ids::{HeadingKey, LabelId};
pub(crate) use policy::SemanticPolicy;
pub(crate) use semantic_document::SemanticDocument;
