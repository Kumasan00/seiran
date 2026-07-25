//! 組版パス統合クレート — Document IR（`model::DocNode`）から計測済み・配置済みページ直前までを担う

mod block;
mod breaking;
mod lowering;

pub use block::{
  IndexEntryInput, IndexPageRef, IndexSpec, RunningContentSpec, RunningMetadata, RunningSlots, TocEntryInput, TocSpec,
  build_blocks, build_index_blocks, build_running_content, build_toc_blocks, sort_index_entries,
};
pub use breaking::{
  BreakKind, BreakPoint, GreedyBreaker, KnuthPlassBreaker, Lang, LineBreaker, PageGeometry, break_opportunities,
  break_pages, resolve_hyphenation,
};
pub use lowering::{
  HeadingRecord, LayoutNode, LoweringContext, LoweringError, MathBlockRow, SourceGroup, TableCellLayout, TableLayout,
  TableRowLayout, TextStyle, TitlePageMetadata, lower_document, lower_nodes, lower_sources_with_headings,
  lower_title_page, per_page_footnote_numbers,
};
pub use model::{TableColumn, column_width};
