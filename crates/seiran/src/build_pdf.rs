//! PDF を生成するモジュール
//! このモジュールは、テキストファイルから PDF を生成するための主要な機能を提供します。

#![allow(unused_assignments)]

use std::path::{Path, PathBuf};

use font::{
  FontDataExt, FontRefsExt, font_info,
  font_info::FontInfosExt,
  glyph_mapping,
  glyph_mapping::GlyphMappingsExt,
  shaper,
  shaper::{HarfRustShapersExt, ShaperDatasExt, ShaperInstancesExt},
  subset, validate_font,
};
use layout::layout_engine;
use miette::Diagnostic;
use thiserror::Error;
use tracing::info;

/// PDF ビルド時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum BuildPdfError {
  /// PDF ファイルの書き込みに失敗した場合
  #[error("PDF ファイルの保存に失敗しました: {path}")]
  #[diagnostic(code(build::write_pdf), help("出力ディレクトリが存在し、書き込み権限があることを確認してください。"))]
  WritePdf {
    /// 出力パス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// TeX テキストから PDF を生成
///
/// # Arguments
///
/// * `file_path` - 入力テキストファイルのパス
/// * `config_path` - 設定ファイルのパス
pub(super) fn build_pdf(file_path: &Path, config_path: &PathBuf) -> miette::Result<()> {
  info!(file_path = %file_path.display(), "PDF のビルドを開始します");

  let config = read_config::read_config(config_path)?;
  let _style = read_style::read_style(config.style_path.as_deref())?;
  let _references = read_references::read_references(config.references_path.as_deref())?;

  let doc_nodes = parser::text_parser(file_path)?;
  info!("テキストのパースが完了しました");

  let lowering_ctx = layout::LoweringContext::new(config.pdf.font_size);
  let layout_nodes = layout::lower_nodes(&lowering_ctx, &doc_nodes);
  println!("Layout Nodes: {layout_nodes:#?}");
  info!("Document IR → LayoutNode への変換が完了しました");

  let font_data = font::FontData::new(&config.font_configs)?;
  info!("フォントの読み込みが完了しました");

  let font_refs = font::FontRefs::new(&config.font_configs, &font_data)?;

  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!("フォントの検証が完了しました");

  let shaper_datas = shaper::ShaperDatas::new(&font_refs);
  let shaper_instances = shaper::ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers =
    shaper::HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  info!("シェーパーの初期化が完了しました");

  let font_infos = font_info::FontInfos::new(&config.font_configs, &font_refs)?;
  let mut glyph_mappings = glyph_mapping::GlyphMappings::new(&font_infos);

  let items = layout_engine(layout_nodes, &harf_rust_shapers, &font_refs, &font_infos, &mut glyph_mappings)?;
  info!("レイアウトの計算が完了しました");

  let subset_bytes = subset::create_font_subset(&config.font_configs, &font_data, &glyph_mappings)?;

  let pdf_bytes = pdf_gen::pdf_gen(&config, &subset_bytes, &items, &font_infos, &glyph_mappings);

  std::fs::write(&config.pdf.output_path, pdf_bytes).map_err(|source| BuildPdfError::WritePdf {
    path: config.pdf.output_path.display().to_string(),
    source,
  })?;
  info!(output_path = %config.pdf.output_path.display(), "PDF の保存が完了しました");

  return Ok(());
}
