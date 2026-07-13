//! テキストソースから CST（具象構文木）を生成する
//!
//! TeX スタイルのソーステキストを字句解析・構文解析し、`bumpalo::Bump`
//! アリーナ上にロスレスな CST を構築します。クレート内部の実装詳細であり、
//! `frontend` クレートの外には公開されません（公開 API は [`crate::parse_source`] のみ）。
//!
//! ## 処理パイプライン
//!
//! ```text
//! ソーステキスト
//!   ↓ [lexer]     トークン列に分割
//! Token 列 (Copy)
//!   ↓ [parser]    アリーナベース CST を構築
//! CST (green::GreenNode) — bumpalo::Bump アリーナ上
//! ```
//!
//! ## モジュール構成
//!
//! - [`span`] — ソース位置 `Span` の定義（共通基盤）
//! - [`token`] — トークン型の定義（lexer の出力 / parser の入力語彙）
//! - [`cst`] — CST の表現（`kind` / `green` / `ast` を束ねる出力データモデル）
//!
//! `cst` 配下の `green` / `ast` / `kind::SyntaxKind` はルートで再エクスポートし、
//! `syntax::green::*` / `syntax::ast::*` / `syntax::SyntaxKind` のパスを維持します。

mod cst;
mod lexer;
mod parser;
pub mod span;
pub mod token;

pub use cst::{ast, green, kind, kind::SyntaxKind};
pub use parser::{ParseMode, ParserError, parse};
