//! CSL を読まずに走査だけを行う意味解析のテストヘルパ

use crate::{
  document::HirDocument,
  semantics::{GeneratedCitations, References, SemanticDocument, SemanticFailures, SemanticPolicy, fact_collection},
};

/// CSL を読まずに走査だけを行い、[`SemanticDocument`] を組み立てる。
///
/// # Errors
///
/// 重複ラベル・未解決参照・未定義引用キーがある場合にエラーを返す。
pub(crate) fn analyze_for_test(
  document: HirDocument,
  policy: &SemanticPolicy,
  references: &References,
) -> Result<SemanticDocument, SemanticFailures> {
  let facts = fact_collection::collect_facts(&document, policy, references)?;
  return Ok(SemanticDocument::new(document, facts, GeneratedCitations::default()));
}
