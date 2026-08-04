//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 内部の段（parse / resolve / typeset / publication 化）は `build_pdf` module に閉じ、
//! 外へ公開しない（[`Publication`] を除く）。

mod build_pdf;
mod citation;
mod config;
mod font;
mod frontend;
// 旧 model crate の公開 API をそのまま維持して非公開 module として吸収した（#307）。crate 外から
// 到達できなくなったことで、当時は「外部が使い得る」ため clippy::trivially_copy_pass_by_ref や
// dead_code の対象外だった項目（garde カスタムバリデータの固定シグネチャ・汎用ユーティリティ
// メソッド群）が有効化前提の項目に付く。データモデル変更になるため個々の項目は書き換えず
// module 単位で dead_code のみ抑制する（trivially_copy_pass_by_ref は各項目のシグネチャ変更の
// 要否が個別に異なるため、モジュール単位ではなく該当関数へ個別に allow を付けている）。
// 未使用の再エクスポート（unused_imports）は model.rs の該当 pub use 個別に allow を付けており、
// ここでは抑制しない（他の移設ファイルは通常どおり lint される）。
#[allow(dead_code)]
mod model;
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
// `compile` は `ProjectSource` を境界とするジェネリック関数（`compile<S: ProjectSource>(source: &S,
// root: &ProjectPath) -> ...`）であり、`ProjectSource`/`ProjectPath` は入力型そのもの。
// `FilesystemProjectSource`/`MemoryProjectSource` は呼び出し元（CLI bin target・統合テスト）が
// 有効な入力を組み立てるための唯一の実装 2 種。`SourceReadError` は `ProjectSource::read_text`/
// `read_bytes` の戻り値型（`Result<_, SourceReadError>`）に現れるため、`ProjectSource` を
// 名指しして自前実装しようとする外部呼び出し元がシグネチャに書けなければならない
// （再エクスポートしないと `ProjectSource` trait 自体が事実上実装不能になる）。`Config`/`Style`
// 等の内部データモデルは `compile` の引数にも `Compilation` の出力にも現れない（`ProjectSource`
// 経由でファイルから読み込まれ内部で完結する）ため、ここには含めない。
#[doc(hidden)]
pub use config::test_support;
pub use config::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, ProjectSource, SourceReadError};
pub use pdf_gen::Publication;
