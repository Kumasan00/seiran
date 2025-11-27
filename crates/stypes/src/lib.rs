pub struct PdfOptions<'a> {
  pub output_path: std::path::PathBuf,
  pub font_name: &'a str,
  pub font_size: f32,
  pub page_size: (f32, f32),
}
