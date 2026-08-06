//! 意味解析（[`analyze`]）と、その成果物から組版入力を組み立てる橋渡し（[`build_resolved_document`]）。
//!
//! [`analyze`] が HIR を 1 回走査してラベル宣言・カウンタ構造値・見出し・`\ref` の解決・引用箇所を
//! `SemanticFacts` として確定し、[`AnalyzedDocument`] が HIR と束ねて保持する。表示文字列の生成
//! （`number_format` 等 style 依存）は typeset 側の責務で、ここでは一切行わない。
//!
//! [`build_resolved_document`] は fact を読んで `ResolvedDocument` へ写すだけの一時的な足場で、
//! lowering が [`AnalyzedDocument`] を直接読むようになる #325 で消える。

mod analyze;
mod bridge;
mod counter;
mod document;
mod error;
mod facts;
mod inline;
mod node;

pub use analyze::analyze;
pub use bridge::build_resolved_document;
pub use counter::{CounterKind, CounterValue};
// `ResolvedGenerated`/`ResolvedGroup`/`ResolvedHeading` は `ResolvedDocument` のフィールド型
// としてのみ現れ、型名を名指しする消費者は `typeset` の `#[cfg(test)]` テストだけ。
#[allow(unused_imports)]
pub use document::{ResolvedDocument, ResolvedGenerated, ResolvedGroup, ResolvedHeading};
pub use error::{SemanticError, UnknownCitationSite};
// `HeadingFacts` は `AnalyzedDocument::headings` の要素型としてのみ現れ、型名を名指しする
// 消費者が現状ない。
#[allow(unused_imports)]
pub use facts::{AnalyzedDocument, HeadingFacts};
// `IndexKey` は旧 resolve crate の公開 API 保持のための再エクスポートで、crate::resolve root
// 経由の利用者は `crate::typeset::lowering::inline` の `#[cfg(test)]` テストのみ。cfg(test) を
// 有効化しない通常ビルドでは未使用に見えるため、この pub use にだけ抑制を付ける。
#[allow(unused_imports)]
pub use inline::{IndexKey, ResolvedInline};
// `ResolvedProofTarget` は seiran 内に crate::resolve root 経由の利用者が現状なく（resolve 内部の
// bridge からのみ参照）、`ResolvedTableCell` は `crate::typeset::lowering::table` の
// `#[cfg(test)]` テストのみが利用する。いずれも通常ビルドでは未使用に見えるため、この pub use に
// だけ抑制を付ける（API 面の維持のため再エクスポート自体は残す）。
#[allow(unused_imports)]
pub use node::{
  ResolvedListItem, ResolvedMathRow, ResolvedNode, ResolvedProofTarget, ResolvedTableCell, ResolvedTableRow,
};
