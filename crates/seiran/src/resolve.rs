//! 意味解析（[`analyze`]）— 著者が書いた HIR から「判明した事実」を確定する段。
//!
//! [`analyze`] が HIR を 1 回走査してラベル宣言・カウンタ構造値・見出し・`\ref` の解決・引用箇所を
//! `SemanticFacts` として確定し、[`AnalyzedDocument`] が HIR と束ねて保持する。文書木は読み取り
//! 専用で書き戻さない。表示文字列の生成（`number_format` 等 style 依存）は typeset 側の責務で、
//! ここでは一切行わない。
//!
//! 利用側（`citation` の CSL 整形と `typeset` の lowering）は collection 構造を知らず、
//! [`AnalyzedDocument`] の目的別 query 経由でのみ fact を参照する。
//!
//! [`analyze`] の後に初めて成立する意味上の識別子（[`LabelId`] / [`HeadingKey`]）も本 module が
//! 所有する（`ids` 子 module、#334）。組版側はこれを到達先の名前空間として使うだけで発行しない。

mod analyze;
mod counter;
mod error;
mod facts;
mod ids;

pub use analyze::analyze;
pub use counter::{CounterKind, CounterValue};
pub use error::{SemanticError, UnknownCitationSite};
pub use facts::AnalyzedDocument;
pub use ids::{HeadingKey, LabelId};
