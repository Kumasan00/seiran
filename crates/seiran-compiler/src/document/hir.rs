//! 著者が書いた内容を表す文書木（HIR）。
//!
//! 全ノード（block / inline / math）が [`NodeId`] を持ち、ソース位置は各 variant ではなく
//! [`SourceMap`] に集約する。
mod builder;
mod id;
mod inline;
mod math;
mod node;
mod node_map;
mod source_map;
mod tree;

pub(crate) use builder::HirBuilder;
pub(crate) use id::NodeId;
pub(crate) use inline::{HirInline, HirInlineKind};
pub(crate) use math::{HirMath, HirMathKind, HirMathRow};
pub(crate) use node::{
  HirFigure, HirHeading, HirList, HirListItem, HirMathBlock, HirNode, HirNodeKind, HirProofTarget, HirQuote, HirTable,
  HirTableCell, HirTableRow, HirTheorem,
};
pub(crate) use node_map::NodeMap;
pub(crate) use source_map::{SourceLocation, SourceMap, SourceSpans};
pub(crate) use tree::{HirDocument, HirSource};
