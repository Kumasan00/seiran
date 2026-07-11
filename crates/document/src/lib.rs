//! Document IR（中間表現）の型定義
//!
//! `parser` の Evaluator が CST から生成する**論理的なドキュメント構造**を表現します。
//! セマンティック（意味）情報を保持し、物理レイアウト情報は含みません。
//! `parser`（生産者）と `lowering`（消費者）の双方が依存する共有の契約クレートです。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [Lexer → Parser]
//! CST (GreenNode — bumpalo::Bump アリーナ上)
//!   ↓ [Evaluator]
//! Document IR (DocNode, InlineNode)  ← このクレート
//!   ↓ [lowering]
//! LayoutNode (物理レイアウト)
//!   ↓ [layout::build_blocks → hlist::break_pages]
//! Vec<Page> (確定座標)
//!   ↓ [pdf_gen::render_pages]
//! PDF bytes
//! ```

mod block;
mod caption;
mod inline;
mod list;
mod math;
mod quote;
mod table;

pub use block::{DocNode, Document, ProofTarget, heading_anchor_key};
pub use caption::CaptionPosition;
pub use inline::{InlineNode, inline_nodes_to_plain_text, try_inline_nodes_to_plain_text};
pub use list::ListItem;
pub use math::{MathNode, MathRow, MathStyle};
pub use quote::QuoteKind;
pub use table::{TableCell, TableRow};
pub use types::{ColumnAlign, ColumnWidth, HeadingLevel, Length, MathDelimiter, MathEnvKind, TheoremClass};
