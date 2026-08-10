//! 意味解析（[`analyze`]）— 著者が書いた HIR から「判明した事実」と引用の生成物を確定する段。
//!
//! [`analyze`] が HIR を 1 回走査してラベル宣言・カウンタ構造値・見出し・`\ref` の解決・引用箇所を
//! `SemanticFacts` として確定し（`walk` 子 module）、引用箇所があるときだけ CSL スタイルを読んで
//! 表示と書誌を生成する（`citation` 子 module）。走査 → CSL 整形という順序は module の外からは
//! 見えない。文書木は読み取り専用で書き戻さない。表示文字列の生成（`number_format` 等 style 依存）は
//! typeset 側の責務で、ここでは一切行わない。
//!
//! 利用側（`typeset` の lowering）は collection 構造を知らず、成果物の目的別 query 経由でのみ
//! fact を参照する。
//!
//! [`analyze`] の後に初めて成立する意味上の識別子（[`LabelId`] / [`HeadingKey`]）も本 module が
//! 所有する（`ids` 子 module、#334）。組版側はこれを到達先の名前空間として使うだけで発行しない。

mod analyze;
mod citation;
mod counter;
mod document;
mod error;
mod facts;
mod ids;
mod policy;
mod walk;

pub(crate) use analyze::analyze;
#[cfg(test)]
pub(crate) use analyze::analyze_for_test;
#[cfg(test)]
pub(crate) use citation::test_fixtures;
pub(crate) use citation::{
  CitationFormatError, CitationId, CitationSiteFacts, CitationStyleError, GeneratedBlock, GeneratedCitations,
  GeneratedInline, ReadReferencesError, References, generate_citations, generated_inlines_to_plain_text,
  load_citation_style, read_references,
};
pub(crate) use counter::{CounterKind, CounterValue};
pub(crate) use document::SemanticDocument;
pub(crate) use error::{AnalyzeError, SemanticError, SemanticFailures};
pub(crate) use ids::{HeadingKey, LabelId};
pub(crate) use policy::SemanticPolicy;
