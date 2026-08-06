//! 著者が書いた内容を表す文書木（HIR）。
//!
//! 全ノード（block / inline / math）が [`NodeId`] を持ち、ソース位置は各 variant ではなく
//! [`SourceMap`] に集約する（epic #321 Phase 1、issue #322）。解決済み ID（`LabelId` /
//! `citation::CitationId`）・カウンタ値・CSL 整形結果・Theme 由来の表示文字列は持たない。
//!
//! `NodeId` を発行できるのは `HirBuilder` だけで、発行と同時に位置が記録される。
//! この module の外からは `NodeId` を構築できない（`NodeId::new` / `SourceSpans::alloc` を
//! module 内に閉じている）。
mod builder;
mod document;
mod id;
mod inline;
mod math;
mod node;
mod node_map;
mod source_map;

pub(crate) use builder::HirBuilder;
pub(crate) use document::{HirDocument, HirGroup, HirSource};
pub(crate) use id::NodeId;
pub(crate) use inline::{HirInline, HirInlineKind};
pub(crate) use math::{HirMath, HirMathKind, HirMathRow};
pub(crate) use node::{HirListItem, HirNode, HirNodeKind, HirProofTarget, HirTableCell, HirTableRow};
pub(crate) use node_map::NodeMap;
pub(crate) use source_map::{SourceMap, SourceSpans};
