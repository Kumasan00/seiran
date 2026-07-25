//! パイプライン全段で共有する語彙型、Document IR、組版コア型。
//!
//! 診断や I/O を持たない契約層として、公開 API をクレートルートから提供する。

mod align;
mod block;
mod caption;
mod color;
mod column_width;
mod doc_node;
mod font;
mod font_map;
mod glyph_run;
mod heading_level;
mod hitem;
mod ids;
mod inline;
mod origin;
// garde のカスタムバリデータを名前空間付きで参照するため公開する。
pub mod length;
mod line;
mod link;
mod list;
mod math_class;
mod math_node;
mod page;
mod quote;
mod span;
mod table;
mod table_box;
mod table_column;
mod text_alignment;
mod theorem;

pub use align::Align;
pub use block::{Block, MathRowNumber, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK};
pub use caption::CaptionPosition;
pub use color::Color;
pub use column_width::column_width;
pub use doc_node::{DocNode, Document, ProofTarget};
pub use font::{FontKind, FontType};
pub use font_map::FontMap;
pub use glyph_run::{Glyph, GlyphRun};
pub use heading_level::HeadingLevel;
pub use hitem::{HBox, HBoxContent, HItem, PlacedHItem};
pub use ids::{AssetId, CitationId, FootnoteId, HeadingKey, LabelId};
pub use inline::{InlineNode, inline_nodes_to_plain_text, try_inline_nodes_to_plain_text};
pub use length::Length;
pub use line::{Line, LineFootnote, LineIndexEntry, LineLink, PositionedBox};
pub use link::{AnchorId, AnchorMark, LinkTarget};
pub use list::ListItem;
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_node::{MathNode, MathRow, MathStyle};
pub use origin::{GeneratedOrigin, Origin, SourceId};
pub use page::{
  Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow,
};
pub use quote::QuoteKind;
pub use span::Span;
pub use table::{TableCell, TableRow};
pub use table_box::{
  CellPlacement, RowLink, TableBox, TableCellBox, TableRowBox, collect_row_links, layout_row_cells,
  max_font_size_in_items, measure_items_width, resolve_column_widths, table_row_height,
};
pub use table_column::{ColumnAlign, ColumnWidth, TableColumn};
pub use text_alignment::TextAlignment;
pub use theorem::TheoremClass;
