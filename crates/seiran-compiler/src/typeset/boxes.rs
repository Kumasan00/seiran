//! 組版中間型そのもの — [`Block`] / [`HItem`] / [`Line`] / [`Page`] と表の計測・配置ヘルパ。
//!
//! `block` module（シェーピング + 計測）と `breaking` module（行分割 + 縦組版）の双方から
//! 対称に参照される共有語彙のため、どちらの所有物にもせず本 module に集約する（#280）。
//!
//! 組版時に初めて成立する配置・アンカーの型（[`Align`] / [`FootnoteId`] / [`AnchorId`] /
//! [`AnchorMark`] / [`LinkTarget`]）と、lowering が構築する表レイアウトの入力契約
//! [`TableColumn`] も本 module が所有する（#334）。

mod align;
mod block;
mod hitem;
mod line;
mod link;
mod page;
mod table_box;

pub use align::Align;
pub use block::{Block, MathRowNumber, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK};
pub use hitem::{HBox, HBoxContent, HItem, PlacedHItem};
pub use line::{Line, LineFootnote, LineIndexEntry, LineLink, PositionedBox};
pub use link::{AnchorId, AnchorMark, FootnoteId, LinkTarget};
pub use page::{
  Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow,
};
// セル内容の自然幅は本体コードに消費者がおらず、golden ダンプ（`typeset::dump`）だけが読む。
#[cfg(test)]
pub use table_box::measure_items_width;
pub use table_box::{
  TableBox, TableCellBox, TableColumn, TableRowBox, collect_row_links, layout_row_cells, max_font_size_in_items,
  resolve_column_widths, table_row_height,
};
