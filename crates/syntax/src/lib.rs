//! テキストソースから CST（具象構文木）を生成する
//!
//! TeX スタイルのソーステキストを字句解析・構文解析し、`bumpalo::Bump`
//! アリーナ上にロスレスな CST を構築します。
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
//! - [`kind`] — `SyntaxKind` 列挙型の定義
//! - [`span`] — ソース位置 `Span` の定義
//! - [`token`] — トークン型の定義
//! - [`green`] — アリーナベース CST の型定義（`GreenNode`, `GreenElement`）
//! - [`ast`] — CST 上の型付きビュー（`CommandView`, `EnvironmentView`）

pub mod ast;
pub mod green;
pub mod kind;
mod lexer;
mod parser;
pub mod span;
pub mod token;

pub use kind::SyntaxKind;
pub use parser::{ParserError, parse};
