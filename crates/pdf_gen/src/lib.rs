//! 組版済みの [`Publication`] から PDF を生成する。

mod error;
mod font;
mod image;
mod metadata;
mod publication;
mod render;
mod resources;

use krilla::Document;
use tracing::debug;

pub use crate::{
  error::PdfGenError,
  image::natural_image_size,
  publication::{
    Destination, PaintOp, Point, Publication, PublicationLink, PublicationLinkTarget, PublicationMetadata,
    PublicationOutlineEntry, PublicationPage, Rect,
  },
  resources::ResourceBundle,
};
use crate::{metadata::build_metadata, render::render_pages};

/// [`Publication`] から PDF バイト列を生成する。
///
/// フォント・画像資源は `publication.resources` から取り、これ以外のファイル I/O・
/// フォント資源の構築は行わない。
///
/// # Errors
///
/// 描画要素の生成、PDF の最終化に失敗した場合は [`PdfGenError`] を返す。
pub fn render(publication: &Publication) -> Result<Vec<u8>, PdfGenError> {
  let mut document = Document::new();
  document.set_metadata(build_metadata(&publication.metadata));
  render_pages(&mut document, publication)?;
  let pdf_bytes = document.finish().map_err(|source| return PdfGenError::FinalizeDocument { source })?;
  debug!(page_count = publication.pages.len(), "PDF 描画が完了しました");
  return Ok(pdf_bytes);
}
