use std::path::Path;

use font::{font_info, glyph_mapping, shaper, subset, validate_font};
use miette::IntoDiagnostic;
use parser::layout_engine;
use tracing::info;

/// PDFを生成する
///
/// 設定ファイルとテキストファイルを読み込み、フォント処理を行い、
/// PDFドキュメントを生成します。
///
/// # 引数
///
/// * `file_path` - 入力テキストファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイル読み込み、フォント処理、PDF生成のいずれかで失敗した場合。
pub(super) fn build_pdf(file_path: &Path) -> miette::Result<()> {
  info!(text_file_path = %file_path.display(), "Input text file path");
  let config = read_config_file::read_config_file()?;
  info!(config.name, "Document name");

  let layout_nodes = parser::text_parser(file_path, &config)?;
  let start = std::time::Instant::now();
  let font_data = font::FontData::new(&config.font_configs)?;
  let duration = start.elapsed();
  info!(duration = ?duration, "Font data loaded");
  let font_refs = font::FontRefs::new(&config.font_configs, &font_data)?;

  for (font_type, font_config) in &config.font_configs {
    info!(font_path = %font_config.font_path.display(), ?font_type, "Validating font");
    let font_ref = font_refs.get(font_type);
    validate_font::validate_font(font_config, font_ref)?;
    info!(font_path = %font_config.font_path.display(), ?font_type, "Font validated successfully");
  }

  let shaper_datas = shaper::ShaperDatas::new(&font_refs);
  let shaper_instances = shaper::ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers =
    shaper::HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  let font_infos = font_info::FontInfos::new(&config.font_configs, &font_refs)?;

  let mut glyph_mappings = glyph_mapping::GlyphMappings::new(&font_infos);
  let items =
    layout_engine::layout_engine(layout_nodes, &harf_rust_shapers, &font_refs, &font_infos, &mut glyph_mappings)?;

  let subset_bytes = subset::create_font_subset(&config.font_configs, &font_data, &glyph_mappings)?;

  let pdf_bytes = pdf_gen::pdf_gen(&config, &subset_bytes, items, &font_infos, &glyph_mappings);

  std::fs::write(&config.pdf.output_path, pdf_bytes).into_diagnostic()?;
  info!(pdf_path = %config.pdf.output_path.display(), "PDF generated successfully");

  Ok(())
}
