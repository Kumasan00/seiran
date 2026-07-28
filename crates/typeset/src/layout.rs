//! 組版中間型そのもの — [`Block`] / [`HItem`] / [`Line`] / [`Page`] と表の計測・配置ヘルパ。
//!
//! `block` module（シェーピング + 計測）と `breaking` module（行分割 + 縦組版）の双方から
//! 対称に参照される共有語彙のため、どちらの所有物にもせず本 module に集約する（#280）。

mod block;
mod hitem;
mod line;
mod page;
mod table_box;

pub use block::{Block, MathRowNumber, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK};
pub use hitem::{HBox, HBoxContent, HItem, PlacedHItem};
pub use line::{Line, LineFootnote, LineIndexEntry, LineLink, PositionedBox};
pub use page::{
  Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow,
};
pub use table_box::{
  CellPlacement, TableBox, TableCellBox, TableRowBox, collect_row_links, layout_row_cells, max_font_size_in_items,
  measure_items_width, resolve_column_widths, table_row_height,
};
