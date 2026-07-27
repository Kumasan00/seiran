//! 組版パス統合クレート — Document IR（`model::DocNode`）から計測済み・配置済みページ直前までを担う
//!
//! 組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）は本クレート非公開 module
//! `layout` が所有する（#280）。

mod block;
mod breaking;
mod layout;
mod lowering;

pub use block::{
  IndexEntryInput, IndexPageRef, IndexSpec, RunningContentSpec, RunningMetadata, RunningSlots, TocEntryInput, TocSpec,
  build_blocks, build_index_blocks, build_running_content, build_toc_blocks, sort_index_entries,
};
pub use breaking::{
  BreakKind, BreakPoint, GreedyBreaker, KnuthPlassBreaker, Lang, LineBreaker, PageGeometry, break_opportunities,
  break_pages, resolve_hyphenation,
};
pub use font::{Glyph, GlyphRun};
pub use layout::{
  Block, CellPlacement, HBox, HBoxContent, HItem, Line, LineFootnote, LineIndexEntry, LineLink, MathRowNumber,
  PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK, Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedHItem,
  PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow, PositionedBox, RowLink, TableBox, TableCellBox,
  TableRowBox, collect_row_links, layout_row_cells, max_font_size_in_items, measure_items_width, resolve_column_widths,
  table_row_height,
};
pub use lowering::{
  HeadingRecord, LayoutNode, LoweringContext, LoweringError, MathBlockRow, SourceGroup, TableCellLayout, TableLayout,
  TableRowLayout, TextStyle, TitlePageMetadata, lower_document, lower_nodes, lower_sources_with_headings,
  lower_title_page, per_page_footnote_numbers,
};
pub use model::{TableColumn, column_width};
