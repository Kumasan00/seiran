//! パイプライン全段で共有する語彙型と Document IR。
//!
//! 診断や I/O を持たない契約層として、公開 API をクレートルートから提供する。

mod align;
mod caption;
mod color;
mod column_width;
mod doc_node;
mod font;
mod font_map;
mod heading_level;
mod ids;
mod inline;
mod origin;
// garde のカスタムバリデータを名前空間付きで参照するため公開する。
pub mod length;
mod link;
mod list;
mod math_class;
mod math_node;
mod quote;
mod span;
mod table;
mod table_column;
mod text_alignment;
mod theorem;

pub use align::Align;
pub use caption::CaptionPosition;
pub use color::Color;
pub use column_width::column_width;
pub use doc_node::{DocNode, Document, ProofTarget};
pub use font::{FontKind, FontType};
pub use font_map::FontMap;
pub use heading_level::HeadingLevel;
pub use ids::{AssetId, CitationId, FootnoteId, HeadingKey, LabelId};
pub use inline::{InlineNode, inline_nodes_to_plain_text, try_inline_nodes_to_plain_text};
pub use length::Length;
pub use link::{AnchorId, AnchorMark, LinkTarget};
pub use list::ListItem;
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_node::{MathNode, MathRow, MathStyle};
pub use origin::{GeneratedOrigin, Origin, SourceId};
pub use quote::QuoteKind;
pub use span::Span;
pub use table::{TableCell, TableRow};
pub use table_column::{ColumnAlign, ColumnWidth, TableColumn};
pub use text_alignment::TextAlignment;
pub use theorem::TheoremClass;
