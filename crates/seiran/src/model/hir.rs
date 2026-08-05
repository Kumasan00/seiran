//! 著者が書いた内容を表す文書木（HIR）。
//!
//! 全ノード（block / inline / math）が [`NodeId`] を持ち、ソース位置は各 variant ではなく
//! [`SourceMap`] に集約する（epic #321 Phase 1、issue #322）。解決済み ID（`LabelId` /
//! `CitationId`）・カウンタ値・CSL 整形結果・Theme 由来の表示文字列は持たない。
//!
//! `NodeId` を発行できるのは `HirBuilder` だけで、発行と同時に位置が記録される。
//! この module の外からは `NodeId` を構築できない（`NodeId::new` / `SourceSpans::alloc` を
//! module 内に閉じている）。
// 本番経路への接続は #322 の後半 task（`frontend` の HIR 化と `build_pdf` の配線）で行う。
// それまでは未使用の型・メソッドが残るため、この module 単位で抑制する。
#![allow(dead_code)]

mod document;
mod id;
mod inline;
mod math;
mod node;
mod source_map;

pub(crate) use document::{HirDocument, HirGroup, HirSource};
pub(crate) use id::NodeId;
pub(crate) use inline::{HirInline, HirInlineKind};
pub(crate) use math::{HirMath, HirMathKind, HirMathRow};
pub(crate) use node::{HirListItem, HirNode, HirNodeKind, HirProofTarget, HirTableCell, HirTableRow};
pub(crate) use source_map::{SourceLocation, SourceMap, SourceSpans};
