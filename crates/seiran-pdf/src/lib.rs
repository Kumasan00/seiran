//! `seiran-compiler` が確定させた `Publication` を PDF バイト列へ描画する backend。
//!
//! 受け取るのは確定座標の描画命令とフォント・画像の生資源だけで、レイアウトの判断は一切しない
//! （組版は `seiran_compiler::compile` に閉じている）。krilla フォントの構築・画像のデコード・
//! PDF の encode がこの crate の責務（#372）。

mod error;
mod font;
mod image;
mod metadata;
mod render;

use krilla::Document;
use seiran_compiler::Publication;
use tracing::debug;

pub use crate::error::PdfGenError;
use crate::{font::build_krilla_fonts, metadata::build_metadata, render::render_pages};

/// [`Publication`] から PDF バイト列を生成する。
///
/// フォント・画像資源は `publication.resources` から取り、ファイル I/O は一切行わない。
///
/// # Errors
///
/// krilla フォントの構築、描画要素の生成、PDF の最終化に失敗した場合は [`PdfGenError`] を返す。
pub fn render(publication: &Publication) -> Result<Vec<u8>, PdfGenError> {
  let fonts = build_krilla_fonts(&publication.resources)?;
  let mut document = Document::new();
  document.set_metadata(build_metadata(&publication.metadata));
  render_pages(&mut document, publication, &fonts)?;
  let pdf_bytes = document.finish().map_err(|source| return PdfGenError::FinalizeDocument { source })?;
  debug!(page_count = publication.pages.len(), "PDF 描画が完了しました");
  return Ok(pdf_bytes);
}
