//! 組版中間型そのもの — [`Block`] / [`HItem`] / [`Line`] / [`Page`] と、表エンジンが組んだ表の行断片
//! [`TableBox`] / [`TableRowFragment`] / [`TableBoundary`]、副図の並び [`SubfiguresBox`]。
//!
//! 組版時に初めて成立する配置・アンカーの型（[`Align`] / [`FootnoteId`] / [`AnchorId`] /
//! [`LinkTarget`]）も本 module が所有する。

mod align;
mod block;
mod galley;
mod hitem;
mod line;
mod link;
mod page;
mod subfigures_box;
mod table_box;

pub(super) use align::Align;
pub(super) use block::{Block, MathRowNumber, usable_width};
pub(super) use galley::Galley;
pub(crate) use hitem::HBoxContent;
pub(super) use hitem::{FootnoteBody, GlueKind, HBox, HItem, IndexTerm, PlacedHBox};
pub(crate) use line::Line;
pub(super) use line::LineLink;
pub(crate) use link::{AnchorId, FootnoteId, LinkTarget};
pub(crate) use page::{Page, PlacedBlock, PlacedTableRow};
pub(super) use page::{PlacedAnchor, PlacedFootnote, PlacedLink, PlacedMathNumber, PlacedTableRule};
pub(super) use subfigures_box::{SubfigureImage, SubfigureLine, SubfiguresBox};
pub(super) use table_box::{TableBoundary, TableBox, TableRowFragment};
