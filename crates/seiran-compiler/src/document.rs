//! 著者が書いた文書（authored HIR）を所有する module。
//!
//! HIR は frontend の一時的な構文木ではなく、`semantics` と `typeset` が共有する authored 文書の
//! 正典である。producer は frontend 1 つだが、HIR の意味と寿命は frontend の実装より広いので、
//! ここが所有者になる（epic #332）。診断ライブラリ（miette）も I/O も持たない。
//!
//! # 提供する interface
//!
//! - frontend が HIR を構築するための [`HirBuilder`] と HIR ノード型
//! - 複数ソースを決定順序で束ねる組み立て（[`HirSource`] → [`HirGroup`] → [`HirDocument`]）
//! - `semantics` / `typeset` が authored 文書を網羅的に走査するための HIR enum。
//!   網羅的 match は意図した interface で、新しい言語要素を足したときに resolve と lowering の
//!   更新漏れをコンパイラに検出させる
//! - 診断側が [`NodeId`] からソース位置を引く query（[`SourceMap`]）
//!
//! interface に出さないもの: `NodeId` の発行、位置表の内部 collection（`SourceSpans`）、
//! ソース順の正規化。`NodeId` を発行できるのは [`HirBuilder`] だけで、発行と同時に位置が
//! 記録される（`NodeId::new` / `SourceSpans::alloc` は `hir` module 内に閉じている）。
//! side table の [`NodeMap`] も crate 内 interface に留め、`SemanticDocument` や
//! `GeneratedCitations` の外部表現としては公開しない。
//!
//! # 置く型・置かない型
//!
//! HIR 木の型は子 module `hir`、HIR の variant が値として直接持つ閉じた語彙型
//! （[`HeadingLevel`] / [`CaptionPosition`] / [`QuoteKind`] / [`TheoremClass`] / [`MathEnvKind`] /
//! [`MathDelimiter`] / [`MathVariant`] / [`ColumnAlign`] / [`ColumnWidth`] / [`FontKind`]）は
//! この module の直下。
//! 語彙置き場は型の無制限な受け皿にはせず、HIR の variant と同じ理由で増減する語彙だけを置く
//! （複数 consumer が使うことは、ここへ置く理由にならない）。
//!
//! HIR と同形の中間 IR は持たない — 数式の中間型 `MathNode` とその変換（`to_math_nodes`）は、
//! `typeset::lowering` が [`HirMath`] / [`HirMathKind`] を直接読むようにして削除済み（#335）。
//! HIR は未解決のラベル名・引用キーを持ち、解決済み ID（`LabelId` / `citation::CitationId`）・
//! カウンタ値・CSL 整形結果・style 由来の表示文字列は持たない。
//!
//! 依存方向は `source` / `project` / `length` / `color` の 4 つだけで、
//! `semantics` / `typeset` / `compiler` は知らない。

mod caption;
mod font_kind;
mod heading_level;
mod hir;
mod math_class;
mod math_variant;
mod quote;
mod table_column;
mod theorem;

pub use caption::CaptionPosition;
pub use font_kind::FontKind;
pub use heading_level::HeadingLevel;
// HIR（#322）は crate 内部だけで使う型なので `pub(crate)` で再エクスポートする。
pub(crate) use hir::{
  HirBuilder, HirDocument, HirGroup, HirInline, HirInlineKind, HirListItem, HirMath, HirMathKind, HirMathRow, HirNode,
  HirNodeKind, HirProofTarget, HirSource, HirTableCell, HirTableRow, NodeId, NodeMap, SourceMap,
};
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_variant::MathVariant;
pub use quote::QuoteKind;
pub use table_column::{ColumnAlign, ColumnWidth};
pub use theorem::TheoremClass;
