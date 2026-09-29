//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の意味解析・
//! CSL 整形・参考文献リスト（書誌）生成までを 1 module に閉じる。

mod csl_json;
mod csl_style;
mod generate;
mod generated;
mod references;
mod render;
mod site;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use csl_style::{CitationStyleError, load_citation_style};
pub(crate) use generate::{GeneratedCitations, generate_citations};
pub(crate) use generated::{BibliographyEntry, GeneratedInline, generated_inlines_to_plain_text};
pub(crate) use references::{ReadReferencesError, Reference, References, read_references};
pub(crate) use site::{CitationId, CitationSiteFacts};
