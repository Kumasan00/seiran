//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 内部の段（parse / resolve / typeset / publication 化）は `build_pdf` module に閉じ、
//! 外へ公開しない（[`Publication`] を除く）。

mod build_pdf;
mod citation;
mod color;
mod config;
mod font;
mod frontend;
// `length` / `color` は crate root 直下の leaf module（#336）。crate root の非公開 module は
// crate 全体から `crate::length::...` で到達できるため、`model` の子だったときに garde の
// カスタムバリデータ参照のために要った `pub(crate)` は不要になった。
mod length;
mod model;
mod project;
mod resolve;
mod source;
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
pub use project::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, ProjectSource, SourceReadError};
pub use seiran_pdf::Publication;
