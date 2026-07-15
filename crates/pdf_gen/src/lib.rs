//! PDF 生成モジュール
//!
//! 組版済みの `model::Page` 列（確定座標）を Krilla で描画し、PDF バイト列を生成します。
//! レイアウト判断は前段（`typeset` の `block` / `breaking`）で完了しており、本クレートが担うのは
//! 画像サイズ確定の prepass（[`resolve_images`]）と描画（[`create_pdf`]）のみです。
//! フォントサブセット化は krilla が内部で実施します。

mod error;
mod font;
mod image;
mod metadata;
mod render;

use ::font::{FontData, FontMetrics, FontRefs};
use config::{read_config::Config, read_style::Style};
use krilla::{Document, page::PageSettings};
use model::{HeadingLevel, Page};
use tracing::debug;

pub use crate::{error::PdfGenError, image::resolve_images};
use crate::{font::build_krilla_fonts, metadata::build_metadata, render::render_pages};

/// PDF のしおり（アウトライン）1 項目の論理情報
///
/// `seiran` の `build_pdf` が Document IR の見出しから文書順に組み立て、[`create_pdf`] に
/// 渡す。確定座標（ページ index + 座標）は `pdf_gen` 側が各ページの見出しアンカーから
/// 補い、本型が持つレベル・テキストと文書順で 1 対 1 に対応付けてしおりツリーを構築する。
#[derive(Debug, Clone)]
pub struct OutlineEntry {
  /// 見出しレベル（ネストの深さに使う）
  pub level: HeadingLevel,
  /// しおりに表示するテキスト（`"{number} {plain title}"`）
  pub text: String,
}

/// フォント情報を使用して PDF バイト列を生成します。
///
/// # Arguments
///
/// * `config` - PDF 生成設定
/// * `font_bytes` - フォントバイナリ
/// * `font_refs` - 解析済みフォント参照
/// * `metrics` - 全フォント種別の基本メトリクス（upem / ascender / descender）
/// * `pages` - 組版済みページ列（`model::break_pages` の出力）
/// * `style` - スタイル設定
/// * `outline_entries` - PDF しおり用の見出し情報（文書順、見出しアンカーと 1 対 1 対応）
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
  outline_entries: &[OutlineEntry],
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
  render_pages(&mut document, &page_settings, config, metrics, &krilla_fonts, pages, style, outline_entries)?;
  let pdf_bytes = document.finish().map_err(|source| PdfGenError::FinalizeDocument { source })?;
  debug!(page_count = pages.len(), "PDF 描画が完了しました");
  return Ok(pdf_bytes);
}
