//! 組版パス統合クレート — Document IR（`model::DocNode`）から計測済み・配置済みページ直前までを担う
//!
//! 旧 `lowering` / `layout` / `hlist` の 3 クレートを module として統合し（#204。#203 の後続）、
//! module 名は責務基準で `lowering` / `block` / `breaking` に改めた（#206）。
//! 各 module は非公開とし、公開 API はこのファイルの `pub use` で 1 本のパスに揃える
//! （`typeset::lower_document` 等。`typeset::lowering::...` は使わない）。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! document (DocNode)
//!   → [lowering]                         Document IR → LayoutNode 変換
//!   → (a) [block::build_blocks]         シェーピング + 計測 + break 注入 [フォント依存]
//!   → (prepass) pdf_gen::resolve_images  画像の自然寸法から width/height を確定
//!   → (c+d) [breaking::break_pages]         行分割 + 縦組版 [フォント非依存の純粋パス]
//!   → (e) pdf_gen::render_pages          確定座標の描画のみ
//! ```
//!
//! ## module 構成
//!
//! - `lowering`（非公開）— Document IR → `LayoutNode` 変換。採番・`\ref` 解決も担う
//! - `block`（非公開）— (a) `build_blocks`。シェーピング + 計測 + break 注入、ヘッダー/フッター配置・目次生成
//! - `breaking`（非公開）— (b)(c)(d) 行分割・縦組版・ハイフネーション。フォント非依存の純粋パス

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
  HeadingRecord, LayoutNode, LoweringContext, LoweringError, MathBlockRow, SourceId, TableCellLayout, TableLayout,
  TableRowLayout, TextStyle, TitlePageMetadata, lower_document, lower_nodes, lower_sources_with_headings,
  lower_title_page, per_page_footnote_numbers,
};
pub use model::{TableColumn, column_width};
