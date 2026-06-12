//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

mod error;
mod font;
mod image;
mod metadata;
mod render;

use ::font::{FontData, FontMetrics, FontRefs};
use hlist::Page;
use krilla::{Document, page::PageSettings};
use read_config::Config;
use read_style::Style;

pub use crate::{error::PdfGenError, image::resolve_images};
use crate::{font::build_krilla_fonts, metadata::build_metadata, render::render_pages};

/// フォント情報を使用して PDF バイト列を生成します。
///
/// # Arguments
///
/// * `config` - PDF 生成設定
/// * `font_bytes` - フォントバイナリ
/// * `font_refs` - 解析済みフォント参照
/// * `metrics` - 全フォント種別の基本メトリクス（upem / ascender / descender）
/// * `pages` - 組版済みページ列（`hlist::break_pages` の出力）
/// * `style` - スタイル設定
///
/// # Returns
///
/// 生成した PDF のバイト列を返します。
///
/// # Errors
///
/// フォント生成、ページ設定、罫線描画の構築に失敗した場合は [`PdfGenError`] を返します。
pub fn create_pdf(
  config: &Config,
  font_bytes: &FontData,
  font_refs: &FontRefs,
  metrics: &FontMetrics,
  pages: &[Page],
  style: &Style,
) -> Result<Vec<u8>, PdfGenError> {
  let krilla_fonts = build_krilla_fonts(config, font_bytes, font_refs)?;
  let page_width = config.pdf.width.to_pt();
  let page_height = config.pdf.height.to_pt();
  let page_settings = PageSettings::from_wh(page_width, page_height).ok_or(PdfGenError::InvalidPageSize {
    width: page_width,
    height: page_height,
  })?;
  let mut document = Document::new();
  document.set_metadata(build_metadata(config));
  render_pages(&mut document, &page_settings, config, metrics, &krilla_fonts, pages, style)?;
  let pdf_bytes = document.finish().map_err(|source| PdfGenError::FinalizeDocument { source })?;
  return Ok(pdf_bytes);
}
