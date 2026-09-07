//! 画像形式 [`ImageFormat`] と、拡張子からの判定。
//!
//! `PublicationImage` に載って描画バックエンドまで届く描画契約の値型。判定をここ 1 箇所に
//! 置くのは、描画側（`seiran-pdf`）が拡張子を読み直すと同じ判定が 2 つになり食い違いうる
//! ため（#378 で `pdf::unsupported_image_format` を削除した根拠）。呼ぶのは組版の画像資源
//! 解決（`crate::typeset::image`）で、判定済みの値だけが `Publication` に載る。

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
    assert_eq!(ImageFormat::from_path("a.png"), Some(ImageFormat::Png));
    assert_eq!(ImageFormat::from_path("a.JPG"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_path("dir/a.jpeg"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_path("a.svg"), Some(ImageFormat::Svg));
  }

  #[test]
  fn from_path_rejects_unsupported_and_missing_extension() {
    assert_eq!(ImageFormat::from_path("a.gif"), None);
    assert_eq!(ImageFormat::from_path("a"), None);
  }
}
