//! 画像寸法を解決し、組版済みの [`Publication`] から PDF を生成する。

mod error;
mod font;
mod image;
mod metadata;
mod publication;
mod render;

use ::font::{FontData, FontMetrics, FontRefs};
use config::FontConfigs;
use krilla::Document;
use tracing::debug;

pub use crate::{
  error::PdfGenError,
  image::{ImageSet, load_image_set, resolve_images},
  publication::{
    Destination, PaintOp, Point, Publication, PublicationBuilder, PublicationLink, PublicationLinkTarget,
    PublicationMetadata, PublicationOutlineEntry, PublicationPage, Rect,
  },
};
use crate::{font::build_krilla_fonts, metadata::build_metadata, render::render_pages};

/// PDF のしおりに使う見出し。
#[derive(Debug, Clone)]
pub struct OutlineEntry {
  /// 見出しレベル（ネストの深さに使う）
  pub level: model::HeadingLevel,
  /// しおりに表示するテキスト（`"{number} {plain title}"`）
  pub text: String,
}

/// [`Publication`] から PDF バイト列を生成する。
///
/// # Errors
///
/// フォントや描画要素の生成、PDF の最終化に失敗した場合は [`PdfGenError`] を返す。
pub fn create_pdf(
  publication: &Publication,
  font_bytes: &FontData,
  font_refs: &FontRefs,
  metrics: &FontMetrics,
  font_configs: &FontConfigs,
) -> Result<Vec<u8>, PdfGenError> {
  let krilla_fonts = build_krilla_fonts(font_configs, font_bytes, font_refs)?;
  let mut document = Document::new();
  document.set_metadata(build_metadata(&publication.metadata));
  render_pages(&mut document, publication, metrics, &krilla_fonts)?;
  let pdf_bytes = document.finish().map_err(|source| return PdfGenError::FinalizeDocument { source })?;
  debug!(page_count = publication.pages.len(), "PDF 描画が完了しました");
  return Ok(pdf_bytes);
}
