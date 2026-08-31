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

use std::time::Instant;

use krilla::Document;
use seiran_compiler::Publication;
use tracing::info;

pub use crate::error::PdfRenderError;
use crate::{font::build_krilla_fonts, metadata::build_metadata, render::render_pages};

/// [`Publication`] から PDF バイト列を生成する。
///
/// フォント・画像資源は `publication.resources()` から取り、ファイル I/O は一切行わない。
/// 描画命令の値（ページサイズ・矩形・画像参照・到達先ページ）は `Publication` の構築時に
/// 検証済みなので、ここで検査し直さない（#378）。
///
/// # Errors
///
/// krilla フォントの構築、画像のデコード、PDF の最終化に失敗した場合は [`PdfRenderError`] を返す。
pub fn render(publication: &Publication) -> Result<Vec<u8>, PdfRenderError> {
  let stage_start = Instant::now();
  let fonts = build_krilla_fonts(publication.resources())?;
  let mut document = Document::new();
  document.set_metadata(build_metadata(publication.metadata()));
  render_pages(&mut document, publication, &fonts)?;
  let pdf_bytes = document.finish().map_err(|source| return PdfRenderError::FinalizeDocument { source })?;
  info!(
    page_count = publication.pages().len(),
    byte_count = pdf_bytes.len(),
    elapsed = ?stage_start.elapsed(),
    "PDF を描画"
  );
  return Ok(pdf_bytes);
}
