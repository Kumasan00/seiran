use std::{fs, path::Path};

mod pre_config;
use pre_config::PreConfig;
mod processed_config;

pub fn read_config_file() -> Result<processed_config::Config, Box<dyn std::error::Error>> {
  let config_file_path = Path::new("./config/config.toml");
  let config_content = fs::read_to_string(config_file_path)?;
  let pre_config: PreConfig = toml::from_str(&config_content)?;
  let curent_dir = std::env::current_dir()?;

  let config = processed_config::Config {
    name: pre_config.name.clone(),
    pdf: processed_config::PdfConfig {
      output_path: curent_dir
        .join(pre_config.pdf.output_dir)
        .join(format!("{}.pdf", pre_config.name)),
      height: pre_config.pdf.height.parse()?,
      width: pre_config.pdf.width.parse()?,
      font_size: pre_config.pdf.font_size,
    },
    fonts: pre_config
      .fonts
      .into_iter()
      .map(|font| processed_config::FontConfig {
        font_path: curent_dir.join(font.font_path),
        font_index: font.font_index,
      })
      .collect(),
  };
  Ok(config)
}
