//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の CSL 整形・
//! 参考文献リスト（書誌）生成までを 1 module に閉じる。引用箇所の意味解析（どの `\cite` がどのキーを指すか・
//! 未定義キーの検証）は走査（`fact_collection`）が行う。

mod csl_json;
mod csl_style;
mod generate;
mod output;
mod references;
mod render;
mod site;
#[cfg(test)]
pub(crate) mod test_support;

pub(super) use csl_style::{ReadCitationStyleError, load_citation_style};
pub(super) use generate::{GeneratedCitations, generate_citations};
pub(crate) use output::{BibliographyEntry, GeneratedInline, generated_inlines_to_plain_text};
use references::Reference;
pub(crate) use references::{ReadReferencesError, References, load_references};
pub(crate) use site::CitationId;
pub(super) use site::CitationSiteFacts;
