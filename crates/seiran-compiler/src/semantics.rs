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
  BibliographyEntry, CitationId, GeneratedInline, ReadReferencesError, References, generated_inlines_to_plain_text,
  load_references,
};
use citation::{
  CitationSiteFacts, GeneratedCitations, ReadCitationStyleError, generate_citations, load_citation_style,
};
#[cfg(test)]
pub(crate) use counter::CounterAncestor;
pub(crate) use counter::{CounterKind, CounterValue};
use error::SemanticFailures;
pub(crate) use error::{AnalyzeError, SemanticError};
pub(crate) use ids::{HeadingKey, LabelId};
pub(crate) use policy::SemanticPolicy;
pub(crate) use semantic_document::SemanticDocument;
