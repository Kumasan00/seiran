//! Seiran コンパイラのライブラリ facade。
//!
//! 言語処理・意味解決・組版を 1 回の呼び出しに畳んだ [`compile`] が唯一の外部入口。
//! 段の呼び出し順序は `compiler` module に閉じ、各段（`frontend` / `semantics` / `typeset` /
//! `publication`）は非公開の兄弟 module で外へ公開しない。公開するのは [`compile`] とその成果
//! [`Compilation`]（[`Publication`] とそこから到達できる leaf 値型を含む）・失敗型 [`CompileFailure`]・
//! 入力 seam（[`ProjectSource`] とその実装 / [`ProjectPath`] / [`ProjectSourceError`]）・
//! leaf 値型（[`Length`] / [`Color`] とその `FromStr` エラー型 / [`FontType`] / [`VariationAxis`]）だけ（`#[doc(hidden)]` の
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

pub use color::{Color, ParseColorError};
pub use compiler::{Compilation, CompileFailure, DependencyManifest, Warnings, compile};
pub use length::{Length, ParseLengthError};
#[doc(hidden)]
pub use project::test_support;
pub use project::{
  FilesystemProjectSource, FontType, MemoryProjectSource, ProjectPath, ProjectSource, ProjectSourceError, VariationAxis,
};
pub use publication::{
  FontFaceConfig, FontMetric, Glyph, GlyphRun, ImageFormat, ImageRef, PaintOp, Publication, PublicationDestination,
  PublicationFont, PublicationImage, PublicationLink, PublicationLinkTarget, PublicationMetadata,
  PublicationOutlineEntry, PublicationPage, PublicationPoint, PublicationRect, PublicationResources,
};
