use std::path::PathBuf;

#[derive(Debug)]
pub struct Config {
  pub name: String,
  pub pdf: PdfConfig,
  pub fonts: Vec<FontConfig>,
}
#[derive(Debug)]
pub struct FontConfig {
  pub font_path: PathBuf,
  pub font_index: u32,
}
#[derive(Debug)]
pub struct PdfConfig {
  pub output_path: PathBuf,
  pub height: f32,
  pub width: f32,
  pub font_size: f32,
}
