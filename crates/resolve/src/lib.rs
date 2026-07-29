//! 未解決の名前（ラベル名・`\ref` 参照名・引用キー・索引語）を保持できる `SemanticDocument` と、
//! それらが typed ID へ解決済みの `ResolvedDocument` を分離するクレート。
//!
//! citation クレートの後・typeset クレートの前で実行する。カウンタの値（構造）算出もここに
//! 閉じ、表示文字列の生成（`number_format` 等 style 依存）は typeset 側に残す。

mod counter;
mod document;
mod error;
mod inline;
mod node;
mod resolver;

pub use counter::{CounterKind, CounterValue};
pub use document::{ResolvedDocument, ResolvedGroup, ResolvedHeading, SemanticDocument, SemanticGroup};
pub use error::ResolveError;
pub use inline::{IndexKey, ResolvedInline};
pub use node::{ResolvedListItem, ResolvedMathRow, ResolvedNode, ResolvedTableCell, ResolvedTableRow};
