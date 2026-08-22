//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 内部の段（parse / 意味解析 / typeset / publication 化）は `compiler` module に閉じ、
//! 外へ公開しない。公開するのは [`compile`] の成果物 [`Publication`] と、そこから
//! 到達できる leaf 値型（描画バックエンドが名指しする必要のある型）だけ。

mod color;
mod compiler;
mod document;
// `failures` は段をまたいで使う非空集合 `Failures<E>` を持つ leaf module（#376）。誰にも依存せず、
// `Diagnostic` も実装しない（集約は表示単位ではない）ため `pub use` にも載せない。
mod failures;
mod frontend;
// `length` / `color` は crate root 直下の leaf module（#336）。crate root の非公開 module は
// crate 全体から `crate::length::...` で到達できるため、かつて `model` の子だったときに garde の
// カスタムバリデータ参照のために要った `pub(crate)` は不要になった。
mod length;
mod project;
mod publication;
mod semantics;
mod source;
mod style;
mod typeset;

// `compile` は `ProjectSource` を境界とするジェネリック関数（`compile<S: ProjectSource>(source: &S,
// root: &ProjectPath, base_dir: &Path) -> ...`）であり、`ProjectSource`/`ProjectPath` は入力型そのもの。
// `FilesystemProjectSource`/`MemoryProjectSource` は呼び出し元（CLI bin target・統合テスト）が
// 有効な入力を組み立てるための唯一の実装 2 種。`SourceReadError` は `ProjectSource::read_text`/
// `read_bytes` の戻り値型（`Result<_, SourceReadError>`）に現れるため、`ProjectSource` を
// 名指しして自前実装しようとする外部呼び出し元がシグネチャに書けなければならない
// （再エクスポートしないと `ProjectSource` trait 自体が事実上実装不能になる）。`ProjectConfig`/`Style`
// 等の内部データモデルは `compile` の引数にも `Compilation` の出力にも現れない（`ProjectSource`
// 経由でファイルから読み込まれ内部で完結する）ため、ここには含めない。
pub use color::Color;
pub use compiler::{BuildStatistics, Compilation, CompileFailure, DependencyManifest, OutputPlan, Warnings, compile};
pub use length::{Length, ParseLengthError};
#[doc(hidden)]
pub use project::test_support;
pub use project::{
  FilesystemProjectSource, FontType, MemoryProjectSource, ProjectPath, ProjectSource, SourceReadError,
};
// `Publication` から到達できる leaf 値型はすべてここに載せる — 描画バックエンド（`seiran-pdf`）が
// 描画命令を読むために名指しする必要があるため（#372 で型の所有をこちらへ移した）。`Length` /
// `Color` / `FontType` / `GlyphRun` / `Glyph` / `FontMetric` / `FontFaceConfig` はいずれも
// `PaintOp` または描画資源のフィールド型として現れる。逆に `FontMap` / `FontConfigs` /
// `ProjectConfig` / `Style` / `typeset::Page` のような内部データモデル・組版中間型は、
// `Publication` の非公開フィールドの型としてしか現れないので載せない
// （renderer が「確定座標の描画のみ」でいられる防火壁は、この公開範囲の狭さが担っている）。
pub use publication::{
  Destination, ImageRef, PaintOp, Point, Publication, PublicationFont, PublicationImage, PublicationLink,
  PublicationLinkTarget, PublicationMetadata, PublicationOutlineEntry, PublicationPage, PublicationResources, Rect,
};
pub use typeset::{FontFaceConfig, FontMetric, Glyph, GlyphRun, ImageFormat, VariationAxisConfig};
