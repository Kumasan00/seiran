//! 水平リスト・縦リストのコア型とフォント非依存の純粋組版パス
//!
//! テキスト自動折り返しのパス分割アーキテクチャの中核クレートです。
//! `layout::build_blocks` が計測済みの [`Block`] 列を生成した後、本クレートの
//! 純粋パスがレイアウトを確定し、`pdf_gen` は描画だけを行います。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! lowering (Vec<LayoutNode>)
//!   → (a) layout::build_blocks      … シェーピング + 計測 + break 注入 [フォント依存]
//!   → (prepass) pdf_gen::resolve_images … 画像サイズの確定 [ファイル I/O]
//!   → (c+d) hlist::break_pages      … 行分割 + 縦組版 [純粋・このクレート]
//!   → (e) pdf_gen::render_pages     … 描画のみ
//! ```
//!
//! ## モジュール構成
//!
//! - [`hitem`] - 水平リストの最小単位（`HItem` / `HBox` / `Atom`）
//! - [`glyph_run`] - シェーピング済みグリフ列（plain data）
//! - [`table_box`] - 表ボックスと列幅・行高の純粋計測
//! - [`block`] - 文書の縦リスト要素（`Block`）
//! - [`line`] / [`page`] - 行分割・縦組版の出力型
//! - [`break_opportunities`] - (b) ICU による分割可能点の探索（純粋関数）
//! - [`break_lines`] - (c) 貪欲法の行分割（`LineBreaker` / `GreedyBreaker`）
//! - [`break_pages`] - (d) 縦組版（ベースライン送り・改ページ・表分割）

pub mod block;
pub mod break_lines;
pub mod break_opportunities;
pub mod break_pages;
pub mod glyph_run;
pub mod hitem;
pub mod line;
pub mod page;
pub mod table_box;

pub use block::{Block, MathRowNumber};
pub use break_lines::{GreedyBreaker, LineBreaker};
pub use break_opportunities::{BreakKind, BreakPoint, break_opportunities};
pub use break_pages::{PageGeometry, break_pages};
pub use glyph_run::{Glyph, GlyphRun};
pub use hitem::{HBox, HBoxContent, HItem, PlacedHItem};
pub use line::{Line, LineLink, PositionedBox};
pub use page::{Page, PlacedAnchor, PlacedBlock, PlacedLink, PlacedMathNumber, PlacedTableRow};
pub use table_box::{
  TableBox, TableCellBox, TableRowBox, max_font_size_in_items, measure_items_width, resolve_column_widths,
  table_row_height,
};
