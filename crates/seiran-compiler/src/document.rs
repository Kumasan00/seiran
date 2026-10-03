//! 著者が書いた文書（authored HIR）を所有する module。
//!
//! HIR は frontend の一時的な構文木ではなく、`semantics` と `typeset` が共有する authored 文書の
//! 正典である。
//!
//! # 提供する interface
//!
//! - frontend が HIR を構築するための [`HirBuilder`] と HIR ノード型
//! - 複数ソースを決定順序で束ねる組み立て（[`HirSource`] → [`HirDocument`]）
//! - `semantics` / `typeset` が authored 文書を網羅的に走査するための HIR enum。
//!   網羅的 match は意図した interface で、新しい言語要素を足したときに意味解析と lowering の
//!   更新漏れをコンパイラに検出させる
//! - 診断側が [`NodeId`] からソース位置を引く query（[`SourceMap`]）
//!
//! # 置く型
//!
//! HIR 木の型は子 module `hir`、HIR の variant が値として直接持つ閉じた語彙型
//! （[`HeadingLevel`] / [`CaptionPosition`] / [`QuoteKind`] / [`TheoremClass`] / [`MathBlockKind`] /
//! [`GridLayout`] / [`MathDelimiter`] / [`MathVariant`] / [`MathClass`] / [`ColumnAlign`] / [`ColumnWidth`] /
//! [`FontKind`]）はこの module の直下。

mod caption;
mod font_kind;
mod heading_level;
mod hir;
mod math_block;
mod math_class;
mod math_variant;
mod quote;
mod table_column;
mod theorem;

pub(crate) use caption::CaptionPosition;
pub(crate) use font_kind::FontKind;
pub(crate) use heading_level::HeadingLevel;
pub(crate) use hir::{
  HirBuilder, HirDocument, HirFigure, HirHeading, HirInline, HirInlineKind, HirList, HirListItem, HirMath,
  HirMathBlock, HirMathKind, HirMathRow, HirNode, HirNodeKind, HirProofTarget, HirQuote, HirSource, HirTable,
  HirTableCell, HirTableRow, HirTheorem, NodeId, NodeMap, SourceLocation, SourceMap,
};
pub(crate) use math_block::{GridLayout, MathBlockKind, MathDelimiter};
pub(crate) use math_class::MathClass;
pub(crate) use math_variant::MathVariant;
pub(crate) use quote::QuoteKind;
pub(crate) use table_column::{ColumnAlign, ColumnWidth};
pub(crate) use theorem::TheoremClass;
