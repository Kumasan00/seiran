//! パイプライン全段で共有する語彙型と文書木（HIR / Document IR）。
//!
//! 診断や I/O を持たない契約層として、`crate::model` 経由で crate 内の全 module から
//! 参照できる語彙型 + Document IR を提供する非公開 module（#307 で `model` crate を吸収）。
//!
//! 「共有されていること」は所有者の不在であって所有の理由ではない、という判断で epic #332 は
//! この module を解体していく。引用まわりの型（`CitationId` / `CitationSiteFacts` /
//! `GeneratedBlock` / `GeneratedInline`）は `crate::citation` へ（#333）、意味解析の識別子
//! （`LabelId` / `HeadingKey`）は `crate::resolve` へ、配置・アンカーの型（`FootnoteId` /
//! `AnchorId` / `AnchorMark` / `LinkTarget` / `Align` / `TableColumn`）は `crate::typeset` の
//! layout へ、検証済み設定値 `TextAlignment` は `crate::config` の style へ移設済み（#334）。
//! これにより `model` から crate 内の他 module への依存は無くなった。
//!
//! HIR と同形の中間 IR は持たない — 数式の中間型 `MathNode` とその変換（`to_math_nodes`）は、
//! `typeset::lowering` が `HirMath` / `HirMathKind` を直接読むようにして削除済み（#335）。
//!
//! 値概念の型 `Length` / `Color` は crate root 直下の leaf module `crate::length` / `crate::color` へ、
//! フォント分類の型 `FontKind` / `FontType` / `FontMap` は `crate::font` へ、段組みの 1 段幅を求める
//! `column_width` は `crate::config` の layout へ移設済み（#336）。

mod caption;
mod heading_level;
mod hir;
mod ids;
mod math_class;
mod math_style;
mod origin;
mod quote;
mod span;
mod table_column;
mod theorem;

pub use caption::CaptionPosition;
pub use heading_level::HeadingLevel;
// HIR（#322）は crate 内部だけで使う型なので `pub(crate)` で再エクスポートする。
pub(crate) use hir::{
  HirBuilder, HirDocument, HirGroup, HirInline, HirInlineKind, HirListItem, HirMath, HirMathKind, HirMathRow, HirNode,
  HirNodeKind, HirProofTarget, HirSource, HirTableCell, HirTableRow, NodeId, NodeMap, SourceMap,
};
pub use ids::AssetId;
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_style::MathStyle;
pub use origin::SourceId;
pub use quote::QuoteKind;
pub use span::Span;
pub use table_column::{ColumnAlign, ColumnWidth};
pub use theorem::TheoremClass;
