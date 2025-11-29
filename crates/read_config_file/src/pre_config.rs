use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub(crate) struct PreConfig {
  pub name: String,
  pub pdf: PrePdfConfig,
  pub main_font: PreFontConfig,
  pub main_japanese_font: PreFontConfig,
}
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfig {
  pub font_name: String,
  pub font_path: String,
  pub font_index: u32,
}
#[derive(Deserialize, Debug)]
pub(crate) struct PrePdfConfig {
  pub output_dir: String,
  pub height: f32,
  pub width: f32,
  pub font_size: f32,
  pub margin_top: f32,
  pub margin_bottom: f32,
  pub margin_left: f32,
  pub margin_right: f32,
}
