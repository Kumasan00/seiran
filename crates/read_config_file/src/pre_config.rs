use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub(crate) struct PreConfig {
  pub name: String,
  pub pdf: PrePdfConfig,
  pub fonts: Vec<PreFontConfig>,
}
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfig {
  pub font_path: String,
  pub font_index: u32,
}
#[derive(Deserialize, Debug)]
pub(crate) struct PrePdfConfig {
  pub output_dir: String,
  pub height: String,
  pub width: String,
  pub font_size: f32,
}
