//! PDF を生成するモジュール
//! このモジュールは、テキストファイルから PDF を生成するための主要な機能を提供します。

use std::path::Path;

use font::{
  FontDataExt, FontRefsExt, font_info, font_info::FontInfosExt, glyph_mapping, glyph_mapping::GlyphMappingsExt,
  shaper, shaper::ShaperDatasExt, shaper::ShaperInstancesExt, shaper::HarfRustShapersExt, subset, validate_font,
};
use miette::IntoDiagnostic;
use layout::layout_engine;
use tracing::info;

/// TeX テキストから PDF を生成
///
/// # Arguments
///
/// * `file_path` - 入力テキストファイルのパス
pub(super) fn build_pdf(file_path: &Path) -> miette::Result<()> {
  info!(file_path = %file_path.display(), "PDF のビルドを開始します");

  let config = read_config::read_config()?;

  let layout_nodes = parser::text_parser(file_path, &config)?;
  info!("テキストのパースが完了しました");

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

  let items =
    layout_engine(layout_nodes, &harf_rust_shapers, &font_refs, &font_infos, &mut glyph_mappings)?;
  info!("レイアウトの計算が完了しました");

  let subset_bytes = subset::create_font_subset(&config.font_configs, &font_data, &glyph_mappings)?;

  let pdf_bytes = pdf_gen::pdf_gen(&config, &subset_bytes, &items, &font_infos, &glyph_mappings);

  std::fs::write(&config.pdf.output_path, pdf_bytes).into_diagnostic()?;
  info!(output_path = %config.pdf.output_path.display(), "PDF の保存が完了しました");

  return Ok(());
}
