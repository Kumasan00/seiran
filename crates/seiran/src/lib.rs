//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 内部の段（parse / resolve / typeset / publication 化）は `build_pdf` module に閉じ、
//! 外へ公開しない（[`Publication`] を除く）。

mod build_pdf;
// 旧 typeset crate の公開 API をそのまま維持して非公開 module として吸収した（#307）。
// crate 外から到達できなくなったことで、当時は「外部が使い得る」ため免除されていた
// dead_code / unused_imports / missing_docs_in_private_items / enum_variant_names が
// 有効化前提の項目（未使用の再エクスポート・対称性のための未読フィールド・
// リネームすると破壊的変更になる列挙子名）に付く。個々の項目を書き換えず module 単位で抑制する。
#[allow(dead_code, unused_imports, clippy::missing_docs_in_private_items, clippy::enum_variant_names)]
mod typeset;

pub use build_pdf::{BuildStatistics, Compilation, DependencyManifest, DiagnosticSet, OutputPlan, compile};
pub use pdf_gen::Publication;
