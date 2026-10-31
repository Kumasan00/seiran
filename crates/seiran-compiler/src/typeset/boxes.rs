//! 組版中間型そのもの — [`Block`] / [`HItem`] / [`Line`] / [`Page`] と、表エンジンが組んだ表の行断片
//! [`TableBox`] / [`TableRowFragment`] / [`TableBoundary`]。
//!
//! 組版時に初めて成立する配置・アンカーの型（[`Align`] / [`FootnoteId`] / [`AnchorId`] /
//! [`LinkTarget`]）も本 module が所有する。

mod align;
mod block;
mod hitem;
mod line;
mod link;
mod page;
mod table_box;

pub(super) use align::Align;
pub(super) use block::{Block, MathRowNumber, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK, usable_width};
pub(crate) use hitem::HBoxContent;
pub(super) use hitem::{HBox, HItem, IndexTerm, MeasuredFootnote, PlacedHBox};
pub(super) use line::{Line, LineLink};
pub(crate) use link::{AnchorId, FootnoteId, LinkTarget};
pub(crate) use page::{Page, PlacedBlock, PlacedTableRow};
pub(super) use page::{PlacedAnchor, PlacedFootnote, PlacedLink, PlacedMathNumber, PlacedTableRule};
pub(super) use table_box::{TableBoundary, TableBox, TableRowFragment};
