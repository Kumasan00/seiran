//! 画像形式の判定 — 拡張子から [`ImageFormat`] を決める唯一の場所。
//!
//! 判定結果は自然寸法の取得（`super::natural_size`）と `Publication` の描画資源の双方が使う。
//! 描画側（`seiran-pdf`）が拡張子を読み直さないのは、同じ判定を 2 回書くと両者が食い違いうる
//! ため（#378 で `pdf::unsupported_image_format` を削除した根拠）。

use std::path::Path;

/// 対応している画像形式。
///
/// `Publication` に載って描画バックエンドまで届く leaf 値型（`crate::publication::PublicationImage`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
  fn from_path_classifies_supported_extensions_case_insensitively() {
    // Arrange / Act / Assert
    assert_eq!(ImageFormat::from_path("a.png"), Some(ImageFormat::Png));
    assert_eq!(ImageFormat::from_path("a.JPG"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_path("dir/a.jpeg"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_path("a.svg"), Some(ImageFormat::Svg));
  }

  #[test]
  fn from_path_rejects_unsupported_and_missing_extension() {
    // Arrange / Act / Assert
    assert_eq!(ImageFormat::from_path("a.gif"), None);
    assert_eq!(ImageFormat::from_path("a"), None);
  }
}
