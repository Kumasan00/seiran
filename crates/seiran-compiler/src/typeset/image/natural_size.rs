//! 画像バイト列から自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）を得る。
//!
//! デコードのみを行い、width / height の確定（縦横比・本文幅からの推論）は行わない
//! （兄弟 module `resources` の `resolve_images` の責務）。ラスタは寸法ヘッダだけを読み、
//! 描画に使う画像本体のデコードは render（`seiran-pdf`）が別に行う。

use std::{io::Cursor, path::Path};

use image::{ImageFormat, ImageReader};

use crate::typeset::error::TypesetError;

/// 画像バイト列をデコードし、自然寸法を返す。
///
/// `path` は拡張子判定とエラーメッセージにのみ使い、ファイルシステムは読まない
/// （読み込み済みの `bytes` をそのままデコードする）。
///
/// # Errors
///
/// 拡張子が未対応の場合、またはデコードに失敗した場合に [`TypesetError`] を返す。
#[allow(clippy::result_large_err)]
pub(super) fn natural_image_size(path: &str, bytes: &[u8]) -> Result<(f32, f32), TypesetError> {
  let extension = Path::new(path)
    .extension()
    .and_then(|e| return e.to_str())
    .map(str::to_ascii_lowercase)
    .unwrap_or_default();
  return match extension.as_str() {
    "png" => raster_size(path, bytes, ImageFormat::Png),
    "jpg" | "jpeg" => raster_size(path, bytes, ImageFormat::Jpeg),
    "svg" => svg_size(path, bytes),
    _ => Err(TypesetError::UnsupportedImageFormat {
      path: path.to_string(),
    }),
  };
}

/// ラスタ画像のピクセル寸法を寸法ヘッダ（PNG は `IHDR`、JPEG は `SOF`）から読む。
///
/// EXIF の Orientation は適用しない — 描画側（krilla）も寸法ヘッダの値をそのまま使うため、
/// 適用すると組版時の自然寸法と描画時の解釈がずれる。
#[allow(clippy::result_large_err, clippy::cast_precision_loss)]
fn raster_size(path: &str, bytes: &[u8], format: ImageFormat) -> Result<(f32, f32), TypesetError> {
  let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().map_err(|source| {
    return TypesetError::DecodeImage {
      path: path.to_string(),
      source,
    };
  })?;
  return Ok((width as f32, height as f32));
}

/// SVG の width / height を usvg が解釈した値として返す。
#[allow(clippy::result_large_err)]
fn svg_size(path: &str, bytes: &[u8]) -> Result<(f32, f32), TypesetError> {
  let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|source| {
    return TypesetError::ParseSvg {
      path: path.to_string(),
      source,
    };
  })?;
  let size = tree.size();
  return Ok((size.width(), size.height()));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::natural_image_size;
  use crate::typeset::error::TypesetError;

  #[test]
  fn natural_image_size_returns_svg_dimensions_from_bytes() {
    // Arrange
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    // Act
    let size = natural_image_size("icon.svg", svg).expect("有効な SVG はデコードできるはず");

    // Assert
    assert!((size.0 - 80.0).abs() < 1e-4);
    assert!((size.1 - 60.0).abs() < 1e-4);
  }

  #[test]
  fn natural_image_size_reports_unsupported_extension() {
    // Arrange
    let bytes = b"not an image";

    // Act
    let result = natural_image_size("icon.gif", bytes);

    // Assert
    assert!(matches!(result, Err(TypesetError::UnsupportedImageFormat { path }) if path == "icon.gif"));
  }

  #[test]
  fn natural_image_size_reports_broken_raster_as_decode_error() {
    // Arrange — 拡張子は png だが中身が PNG ではない
    let bytes = b"not a png";

    // Act
    let result = natural_image_size("broken.png", bytes);

    // Assert
    assert!(matches!(result, Err(TypesetError::DecodeImage { path, .. }) if path == "broken.png"));
  }
}
