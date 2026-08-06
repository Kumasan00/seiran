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
mod math_class;
mod math_style;
mod quote;
mod span;
mod table_column;
mod theorem;

pub use caption::CaptionPosition;
pub use color::Color;
pub use column_width::column_width;
pub use font::{FontKind, FontType};
pub use font_map::FontMap;
pub use heading_level::HeadingLevel;
// HIR（#322）は crate 内部だけで使う型なので `pub(crate)` で再エクスポートする。
pub(crate) use hir::{
  HirBuilder, HirDocument, HirGroup, HirInline, HirInlineKind, HirListItem, HirMath, HirMathKind, HirMathRow, HirNode,
  HirNodeKind, HirProofTarget, HirSource, HirTableCell, HirTableRow, NodeId, NodeMap, SourceMap,
};
pub use ids::AssetId;
pub use length::Length;
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_style::MathStyle;
pub use origin::SourceId;
pub use quote::QuoteKind;
pub use span::Span;
pub use table_column::{ColumnAlign, ColumnWidth};
pub use theorem::TheoremClass;
