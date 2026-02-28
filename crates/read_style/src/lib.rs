use std::path::Path;

use figment::{
  Figment,
  providers::{Format, Serialized, Toml},
};
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};
use tracing::info;

const STYLE_PATH: &str = "config/style.toml";

#[derive(Debug, Deserialize, Serialize)]
pub struct Style {
  pub font_size: f32,
  pub part: HeadingStyle,
  pub chapter: HeadingStyle,
  pub section: HeadingStyle,
  pub sub_section: HeadingStyle,
  pub paragraph: HeadingStyle,
  pub sub_paragraph: HeadingStyle,
}

impl Default for Style {
  fn default() -> Self {
    return Self {
      font_size: 12.0,
      part: HeadingStyle::new(40.0, 20.0),
      chapter: HeadingStyle::new(25.0, 15.0),
      section: HeadingStyle::new(20.0, 10.0),
      sub_section: HeadingStyle::new(16.0, 10.0),
      paragraph: HeadingStyle::new(14.0, 5.0),
      sub_paragraph: HeadingStyle::new(12.0, 5.0),
    };
  }
}

/// 見出し要素のスタイル設定（フォントサイズと下余白）
#[derive(Debug, Deserialize, Serialize)]
pub struct HeadingStyle {
  pub font_size: f32,
  pub bottom_margin: f32,
}

impl HeadingStyle {
  /// 新しい [`HeadingStyle`] を作成する
  #[must_use]
  const fn new(font_size: f32, bottom_margin: f32) -> Self {
    return Self {
      font_size,
      bottom_margin,
    };
  }
}

/// Reads the style configuration from the style TOML file.
///
/// # Errors
///
/// Returns an error if the configuration file cannot be read or if the
/// configuration values cannot be extracted into a [`Style`] struct.
pub fn read_style<P: AsRef<Path>>(path: Option<P>) -> miette::Result<Style> {
  let figment = Figment::from(Serialized::defaults(Style::default()));
  let figment = if let Some(p) = path {
    info!(style_path = %p.as_ref().display(), "スタイル設定ファイルの読み込みを開始します");
    figment.merge(Toml::file(p))
  } else {
    info!(style_path = STYLE_PATH, "デフォルトのスタイル設定ファイルの読み込みを開始します");
    figment.merge(Toml::file(STYLE_PATH))
  };
  let style: Style = figment.extract().into_diagnostic()?;
  info!(font_size = style.font_size, "スタイル設定ファイルの読み込みが完了しました");
  return Ok(style);
}
