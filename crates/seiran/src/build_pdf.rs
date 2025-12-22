use std::path::Path;

use font::{shaper, validate_font};
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
pub(super) fn build_pdf<P: AsRef<Path>>(file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let config = read_config_file::read_config_file();
  let config = match config {
    Ok(cfg) => cfg,
    Err(e) => {
      eprintln!("{:?}", e);
      std::process::exit(1);
    },
  };
  info!(config.name, "Document name");

  for font_config in &config.font_configs {
    validate_font::validate_font(font_config)?;
  }

  parser::text_parser(&file_path)?;
  let _shapers = shaper::HarfRustShapers::new(&config.font_configs)?;

  // let text_lines = read_file(&file_path)?;
  // let mut font_contexts = FontContexts::new(&config)?;
  // let mut glyph_mappings = GlyphMappings::new();

  // let pdf_content =
  //   text::process_text_lines(text_lines, &mut font_contexts, &mut glyph_mappings, &config)?;

  // let subset_bytes = font_context::create_font_subset(&font_contexts, &glyph_mappings)?;

  // let font_datas = font_data::analyze_subset_font(&subset_bytes)?;

  // font::insert_notdef_advance_widths(&mut glyph_mappings, &font_datas);

  // pdf_gen::pdf_gen(
  //   &subset_bytes,
  //   &font_datas,
  //   &glyph_mappings,
  //   pdf_content,
  //   &config,
  // )?;

  // println!("PDF generated");
  Ok(())
}
