//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 内部の段（parse / resolve / typeset / publication 化）は `build_pdf` module に閉じ、
//! 外へ公開しない（[`Publication`] を除く）。

mod build_pdf;
mod citation;
mod frontend;
mod resolve;
// 旧 typeset crate の公開 API をそのまま維持して非公開 module として吸収した（#307）。
// crate 外から到達できなくなったことで、当時は「外部が使い得る」ため免除されていた
// dead_code（対称性のための未読フィールド・未構築 variant）と enum_variant_names
// （リネームすると破壊的変更になる列挙子名 `Block::MathBlock`）が有効化前提の項目に付く。
// データモデル変更になるため個々の項目は書き換えず module 単位で抑制する。未使用の
// 再エクスポート（unused_imports）はそれが生じている typeset.rs の該当 pub use 個別に
// allow を付けており、ここでは抑制しない（他の移設ファイルは通常どおり lint される）。
#[allow(dead_code, clippy::enum_variant_names)]
mod typeset;

pub use build_pdf::{BuildStatistics, Compilation, DependencyManifest, DiagnosticSet, OutputPlan, compile};
pub use pdf_gen::Publication;
