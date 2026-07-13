//! CST（具象構文木）の表現
//!
//! パーサーが構築する出力データモデルを束ねるモジュールです。`bumpalo::Bump`
//! アリーナ上のロスレスな構文木（[`green`]）、ノード種別の列挙（[`kind`]）、
//! そのうえに被せる型付きビュー（[`ast`]）から構成されます。
//!
//! ## モジュール構成
//!
//! - [`kind`] — `SyntaxKind`（CST ノードの種別）
//! - [`green`] — `GreenNode` / `GreenElement`（アリーナベースの木）
//! - [`ast`] — `CommandView` / `EnvironmentView`（CST 上の型付きビュー）

pub mod ast;
pub mod green;
pub mod kind;
