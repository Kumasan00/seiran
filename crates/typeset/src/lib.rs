//! 組版パス統合クレート — 解決済みドキュメント（`resolve::ResolvedDocument`）から計測済み・
//! 配置済みページ直前までを担う
//!
//! 組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）は本クレート非公開 module
//! `layout` が所有する（#280）。

mod block;
mod breaking;
mod layout;
mod lowering;
mod pipeline;

pub use block::{
  IndexEntryInput, IndexPageRef, RunningContentSpec, RunningMetadata, RunningSlots, TocEntryInput,
  layout_running_content, sort_index_entries,
};
pub use breaking::{GreedyBreaker, KnuthPlassBreaker, LineBreaker, PageGeometry};
pub use layout::{
  Block, CellPlacement, HBox, HBoxContent, HItem, Line, LineFootnote, LineIndexEntry, LineLink, MathRowNumber, Page,
  PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedHItem, PlacedIndexEntry, PlacedLink, PlacedMathNumber,
  PlacedTableRow, PositionedBox, TableBox, TableCellBox, TableRowBox, layout_row_cells, max_font_size_in_items,
  measure_items_width,
};
pub use lowering::{
  HeadingRecord, LayoutNode, LoweringContext, MathBlockRow, TableCellLayout, TableLayout, TableRowLayout, TextStyle,
  lower_sources_with_headings, per_page_footnote_numbers,
};
pub use pipeline::{
  BackMatterInput, BodyLayout, BodyLayoutError, BodyLayoutInput, FrontMatterInput, layout_back_matter, layout_body,
  layout_front_matter,
};
