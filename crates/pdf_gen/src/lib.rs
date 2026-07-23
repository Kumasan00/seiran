//! PDF 生成モジュール
//!
//! 組版済みの `Publication`（確定座標）を Krilla で描画し、PDF バイト列を生成します。
//! レイアウト判断は前段（`typeset` の `block` / `breaking`、および `PublicationBuilder`）で
//! 完了しており、本クレートが担うのは画像サイズ確定の prepass（[`resolve_images`]）と
//! 描画（[`create_pdf`]）のみです。フォントサブセット化は krilla が内部で実施します。

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

/// PDF のしおり（アウトライン）1 項目の論理情報
///
/// `seiran` の `build_pdf` が Document IR の見出しから文書順に組み立て、[`create_pdf`] に
/// 渡す。確定座標（ページ index + 座標）は `pdf_gen` 側が各ページの見出しアンカーから
/// 補い、本型が持つレベル・テキストと文書順で 1 対 1 に対応付けてしおりツリーを構築する。
#[derive(Debug, Clone)]
pub struct OutlineEntry {
  /// 見出しレベル（ネストの深さに使う）
  pub level: model::HeadingLevel,
  /// しおりに表示するテキスト（`"{number} {plain title}"`）
  pub text: String,
}

/// `Publication` から PDF バイト列を生成します。
///
/// # Arguments
///
/// * `publication` - 座標・描画順が確定済みの中間表現
/// * `font_bytes` - フォントバイナリ
/// * `font_refs` - 解析済みフォント参照
/// * `metrics` - 全フォント種別の基本メトリクス（upem / ascender / descender）
/// * `font_configs` - フォント埋め込みに必要な設定（`variation_axes` / `font_index`）
///
/// # Errors
///
/// フォント生成、ページ設定、罫線描画の構築に失敗した場合は [`PdfGenError`] を返します。
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
