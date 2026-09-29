//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 段の呼び出し順序は `compiler` module に閉じ、各段（`frontend` / `semantics` / `typeset` /
//! `publication`）は非公開の兄弟 module で外へ公開しない。公開するのは [`compile`] とその成果
//! [`Compilation`]（[`Publication`] とそこから到達できる leaf 値型を含む）・失敗型 [`CompileFailure`]・
//! 入力 seam（[`ProjectSource`] とその実装 / [`ProjectPath`] / [`SourceReadError`]）・
//! leaf 値型（[`Length`] / [`Color`] とその `FromStr` エラー型 / [`FontType`]）だけ（`#[doc(hidden)]` の
//! `test_support` は統合テスト向けの fixture 経路で、API ではない）。

mod color;
mod compiler;
mod document;
mod failures;
mod frontend;
mod length;
mod phase;
mod project;
mod publication;
mod semantics;
mod source;
mod style;
mod typeset;

// `SourceReadError` は `ProjectSource::read_text` / `read_bytes` の戻り値型に現れるので、再輸出しないと
// 外部から `ProjectSource` を実装できない。
pub use color::{Color, ParseColorError};
pub use compiler::{BuildStatistics, Compilation, CompileFailure, DependencyManifest, Warnings, compile};
pub use length::{Length, ParseLengthError};
#[doc(hidden)]
pub use project::test_support;
pub use project::{
  FilesystemProjectSource, FontType, MemoryProjectSource, ProjectPath, ProjectSource, SourceReadError,
};
// `Publication` から到達できる leaf 値型はすべてここに載せる — 描画バックエンド（`seiran-pdf`）が
// 描画命令を読むために名指しする必要があるため。`ProjectConfig` / `Style` /
// `typeset::Page` のような内部データモデル・組版中間型は載せない（renderer が「確定座標の描画のみ」で
// いられる防火壁は、この公開範囲の狭さが担っている）。
pub use publication::{
  Destination, FontFaceConfig, FontMetric, Glyph, GlyphRun, ImageFormat, ImageRef, PaintOp, Point, Publication,
  PublicationFont, PublicationImage, PublicationLink, PublicationLinkTarget, PublicationMetadata,
  PublicationOutlineEntry, PublicationPage, PublicationResources, Rect, VariationAxisConfig,
};
