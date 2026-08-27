//! 組版中間型そのもの — [`Block`] / [`HItem`] / [`Line`] / [`Page`] と表の計測・配置ヘルパ。
//!
//! `boxing` module（シェーピング + 計測）と `breaking` module（行分割 + 縦組版）の双方から
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

pub(super) use align::Align;
pub(super) use block::{Block, MathRowNumber, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK};
pub(crate) use hitem::{HBox, HBoxContent, HItem, PlacedHItem};
pub(super) use line::{Line, LineFootnote, LineIndexEntry, LineLink, PositionedBox};
pub(crate) use link::{AnchorId, AnchorMark, FootnoteId, LinkTarget};
pub(crate) use page::{
  Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow,
  PlacedTableRule,
};
pub(super) use table_box::{
  TableBox, TableCellBox, TableColumn, TableRowBox, collect_row_links, max_font_size_in_items,
  position_table_row_boxes, resolve_column_widths, table_row_height,
};
