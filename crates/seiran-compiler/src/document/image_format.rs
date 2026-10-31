//! 画像形式 [`ImageFormat`] と、拡張子からの判定。

use std::path::Path;

use strum::Display;

/// 対応している画像形式（`\image` の拡張子が宣言する。表示は大文字の形式名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "UPPERCASE")]
pub enum ImageFormat {
  /// PNG（`.png`）
  Png,
  /// JPEG（`.jpg` / `.jpeg`）
  Jpeg,
  /// SVG（`.svg`）
  Svg,
}

impl ImageFormat {
  /// パスの拡張子（大文字小文字を無視）から画像形式を判定する。未対応の拡張子は `None`。
  pub(crate) fn from_path(path: &str) -> Option<Self> {
    let extension = Path::new(path).extension().and_then(|e| return e.to_str()).map(str::to_ascii_lowercase)?;
    return match extension.as_str() {
      "png" => Some(ImageFormat::Png),
      "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
      "svg" => Some(ImageFormat::Svg),
      _ => None,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::ImageFormat;

  #[test]
  fn from_path_classifies_by_extension_case_insensitively() {
    assert_eq!(ImageFormat::from_path("a.png"), Some(ImageFormat::Png));
    assert_eq!(ImageFormat::from_path("a.JPG"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_path("dir/a.jpeg"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_path("a.svg"), Some(ImageFormat::Svg));
    assert_eq!(ImageFormat::from_path("a.gif"), None);
    assert_eq!(ImageFormat::from_path("a"), None);
  }

  #[test]
  fn display_names_the_format_in_upper_case() {
    assert_eq!(ImageFormat::Png.to_string(), "PNG");
    assert_eq!(ImageFormat::Jpeg.to_string(), "JPEG");
    assert_eq!(ImageFormat::Svg.to_string(), "SVG");
  }
}
