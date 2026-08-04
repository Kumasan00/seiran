//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 内部の段（parse / resolve / typeset / publication 化）は `build_pdf` module に閉じ、
//! 外へ公開しない（[`Publication`] を除く）。

mod build_pdf;

pub use build_pdf::{BuildStatistics, Compilation, DependencyManifest, DiagnosticSet, OutputPlan, compile};
pub use pdf_gen::Publication;
