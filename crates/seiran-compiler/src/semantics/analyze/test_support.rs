//! CSL を読まない走査だけの入口を、意味解析の外（`typeset::lowering` のテスト）へ渡すためのヘルパ
//!
//! 本体経路の `analyze` は `ProjectSource` と CSL スタイルを要求するが、走査結果だけを見るテストは
//! そこを通らない。走査そのものは production と同じ `fact_collection::collect_facts` を呼ぶ。

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
