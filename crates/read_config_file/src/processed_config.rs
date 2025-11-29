use std::path::PathBuf;

#[derive(Debug)]
pub struct Config {
  pub name: String,
  pub pdf: PdfConfig,
  pub main_font: FontConfig,
  pub main_japanese_font: FontConfig,
}
#[derive(Debug)]
pub struct FontConfig {
  pub font_name: String,
  pub font_path: PathBuf,
  pub font_index: u32,
}
#[derive(Debug)]
pub struct PdfConfig {
  pub output_path: PathBuf,
  pub height: f32,
  pub width: f32,
  pub font_size: f32,
  pub margin_top: f32,
  pub margin_bottom: f32,
  pub margin_left: f32,
  pub margin_right: f32,
}
