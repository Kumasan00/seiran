//! 全段共有のデータモデル（共通型・Document IR・組版ボックスモデル）
//!
//! パイプライン全体が参照する契約層を 1 クレートに集約する。旧 `types` / `document` /
//! `hlist`（コア型部分）の 3 クレートを統合したもので、境界が形骸化していた
//! （`types` と `document` の間で同一型に 2 通りの参照パスが混在する等）問題を、
//! 単一の root facade に一本化することで解消する（#203）。
//!
//! ## モジュール構成
//!
//! - **語彙型**（旧 `types`）: `align` / `color` / `font` / `font_map` / `heading_level` /
//!   [`length`] / `link` / `math_class` / `table_column` / `text_alignment` / `theorem` /
//!   `span`。19 フォント種別（[`FontType`]）、単位付き長さ（[`Length`]）、8bit RGB 色
//!   （[`Color`]）などの `Copy` 値型・enum と、その正準変換（`as_str` / `from_name` / serde /
//!   `Display`）・純粋演算のみを持つ。IR ノード・組版データ構造・フォントや I/O など
//!   挙動を持つ処理は置かない。
//! - **Document IR**（旧 `document`）: `caption` / `doc_node` / `inline` / `list` / `math_node`
//!   / `quote` / `table`。`frontend`（生産者）と `lowering`（消費者）双方が依存する
//!   セマンティックな文書構造（[`DocNode`] / [`InlineNode`] / [`MathNode`]）。物理レイアウト
//!   情報は含まない。ソース位置は診断ライブラリに依存しない軽量な [`Span`] で持ち、
//!   `miette::SourceSpan` への変換は消費側（`frontend` の構築点・`lowering` の診断構築点）が担う。
//! - **組版コア型**（旧 `hlist` のコア型部分）: `hitem` / `block` / `glyph_run` / `table_box` /
//!   `line` / `page`。フォント非依存の [`Block`] / [`Page`] / [`HItem`] 等と、表の純粋計測
//!   ヘルパ（[`measure_items_width`] / [`max_font_size_in_items`]）を持つ。行分割・縦組版などの
//!   純粋パス本体（`break_opportunities` / `break_lines` / `break_pages` / `hyphenation`）は
//!   `typeset` クレートの `breaking` module にあり、本クレートの型に依存する。確定ページ列の
//!   決定的テキストダンプ（`dump_pages`）は golden テスト専用の出力ツールで、唯一の消費者
//!   `seiran` 側（`build_pdf::dump`）に置く（#216）。段組みの 1 段あたりの幅を求める純粋計算
//!   （[`column_width`]）も本クレートに置き、`config`（横断バリデーション）と
//!   `typeset::breaking`（実配置）の双方が同じ式を参照する（#214）。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [frontend: Lexer → Parser → Evaluator]
//! Document IR (DocNode, InlineNode, MathNode)  ← このクレート
//!   ↓ [lowering]
//! LayoutNode (物理レイアウト、typeset::lowering)
//!   ↓ [typeset::block::build_blocks]
//! Vec<Block>  ← このクレート
//!   ↓ [typeset::breaking::break_pages]（本クレートの型に依存する純粋パス）
//! Vec<Page>  ← このクレート
//!   ↓ [pdf_gen::render_pages]
//! PDF bytes
//! ```
//!
//! 外部依存は `serde` / `garde` のみ（`miette` 等の重い診断ライブラリ・file I/O は持ち込まない）。

mod align;
mod block;
mod caption;
mod color;
mod column_width;
mod doc_node;
mod font;
mod font_map;
mod glyph_run;
mod heading_level;
mod hitem;
mod inline;
// length は garde 用バリデータ（`length::non_negative` / `length::positive`）を
// モジュール名前空間ごと公開するため pub を維持する
pub mod length;
mod line;
mod link;
mod list;
mod math_class;
mod math_node;
mod page;
mod quote;
mod span;
mod table;
mod table_box;
mod table_column;
mod text_alignment;
mod theorem;

pub use align::Align;
pub use block::{Block, MathRowNumber, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK};
pub use caption::CaptionPosition;
pub use color::Color;
pub use column_width::column_width;
pub use doc_node::{DocNode, Document, ProofTarget, heading_anchor_key};
pub use font::{FontKind, FontType};
pub use font_map::{FontMap, FontMapIter, FontMapIterMut};
pub use glyph_run::{Glyph, GlyphRun};
pub use heading_level::HeadingLevel;
pub use hitem::{HBox, HBoxContent, HItem, PlacedHItem};
pub use inline::{InlineNode, inline_nodes_to_plain_text, try_inline_nodes_to_plain_text};
pub use length::{Length, ParseLengthError};
pub use line::{Line, LineFootnote, LineLink, PositionedBox};
pub use link::{AnchorMark, LinkTarget};
pub use list::ListItem;
pub use math_class::{MathDelimiter, MathEnvKind};
pub use math_node::{MathNode, MathRow, MathStyle};
pub use page::{
  Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedLink, PlacedMathNumber, PlacedTableRow, footnote_anchor_key,
};
pub use quote::QuoteKind;
pub use span::Span;
pub use table::{TableCell, TableRow};
pub use table_box::{
  CellPlacement, RowLink, TableBox, TableCellBox, TableRowBox, collect_row_links, layout_row_cells,
  max_font_size_in_items, measure_items_width, resolve_column_widths, table_row_height,
};
pub use table_column::{ColumnAlign, ColumnWidth, TableColumn};
pub use text_alignment::TextAlignment;
pub use theorem::TheoremClass;
