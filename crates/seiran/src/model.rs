//! パイプライン全段で共有する語彙型と文書木（HIR / Document IR）。
//!
//! 診断や I/O を持たない契約層として、`crate::model` 経由で crate 内の全 module から
//! 参照できる語彙型 + Document IR を提供する非公開 module（#307 で `model` crate を吸収）。
//!
//! 「共有されていること」は所有者の不在であって所有の理由ではない、という判断で epic #332 は
//! この module を解体していく。引用まわりの型（`CitationId` / `CitationSiteFacts` /
//! `GeneratedBlock` / `GeneratedInline`）は `crate::citation` へ移設済み（#333）で、
//! 唯一 `link` だけが `citation` を参照する（詳細はその module doc）。

mod align;
mod caption;
mod color;
mod column_width;
mod font;
mod font_map;
mod heading_level;
mod hir;
mod ids;
mod origin;
// garde のカスタムバリデータを `crate::model::length::non_negative` 等の名前空間付きパスで
// 参照するため、crate 内の他 module（`crate::config` 等、`crate::model` の子孫ではない）
// からも見えるよう `pub(crate)` にする（#307、font crate 吸収時の `font::shaper` と同型）。
pub(crate) mod length;
mod link;
mod math_class;
mod math_node;
mod quote;
mod span;
mod table_column;
mod text_alignment;
mod theorem;

pub use align::Align;
pub use caption::CaptionPosition;
pub use color::Color;
pub use column_width::column_width;
pub use font::{FontKind, FontType};
pub use font_map::FontMap;
pub use heading_level::HeadingLevel;
// HIR（#322）は crate 内部だけで使う型なので `pub(crate)` で再エクスポートする。
pub(crate) use hir::{
  HirBuilder, HirDocument, HirGroup, HirInline, HirInlineKind, HirListItem, HirMath, HirMathKind, HirMathRow, HirNode,
  HirNodeKind, HirProofTarget, HirSource, HirTableCell, HirTableRow, NodeId, NodeMap, SourceMap, to_math_nodes,
};
pub use ids::{AssetId, FootnoteId, HeadingKey, LabelId};
pub use length::Length;
pub use link::{AnchorId, AnchorMark, LinkTarget};
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_node::{MathNode, MathStyle};
pub use origin::SourceId;
pub use quote::QuoteKind;
pub use span::Span;
pub use table_column::{ColumnAlign, ColumnWidth, TableColumn};
pub use text_alignment::TextAlignment;
pub use theorem::TheoremClass;
