//! フォント非依存の純粋組版パス（行分割・縦組版・ハイフネーション）
//!
//! テキスト自動折り返しのパス分割アーキテクチャの中核 module です。コア型
//! （[`model::Block`] / [`model::Page`] / [`model::HItem`] 等）は `model` クレートに移り、
//! 本 module には純粋パス本体だけが残る（#203）。`crate::block::build_blocks` が計測済みの
//! `model::Block` 列を生成した後、本 module の純粋パスがレイアウトを確定し、`pdf_gen` は
//! 描画だけを行う。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! lowering (Vec<LayoutNode>)
//!   → (a) crate::block::build_blocks   … シェーピング + 計測 + break 注入 [フォント依存]
//!   → (prepass) pdf_gen::resolve_images … 画像サイズの確定 [ファイル I/O]
//!   → (c+d) break_pages（この module）   … 行分割 + 縦組版 [純粋]
//!   → (e) pdf_gen::render_pages          … 描画のみ
//! ```
//!
//! ## モジュール構成
//!
//! - `break_opportunities` - (b) ICU による分割可能点の探索（純粋関数）
//! - `hyphenation` - 欧文ハイフネーション（語中分割位置の探索・`hypher`）
//! - `break_lines` - (c) 行分割（[`LineBreaker`] / [`GreedyBreaker`] / [`KnuthPlassBreaker`]）
//! - `break_pages` - (d) 縦組版（ベースライン送り・改ページ・表分割）

mod break_lines;
mod break_opportunities;
mod break_pages;
mod hyphenation;

pub use break_lines::{GreedyBreaker, KnuthPlassBreaker, LineBreaker};
pub use break_opportunities::{BreakKind, BreakPoint, break_opportunities};
pub use break_pages::{PageGeometry, break_pages, column_width};
pub use hyphenation::{Lang, resolve as resolve_hyphenation};
