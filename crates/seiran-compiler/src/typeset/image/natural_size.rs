//! 画像バイト列から自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）を得る。
//!
//! デコードのみを行い、width / height の確定（縦横比・本文幅からの推論）は行わない
//! （兄弟 module `resources` の `resolve_images` の責務）。ラスタは寸法ヘッダだけを読み、
//! 描画に使う画像本体のデコードは render（`seiran-pdf`）が別に行う。

use std::io::Cursor;

use image::ImageReader;

use crate::typeset::{error::TypesetError, image::ImageFormat};

/// 判定済みの画像形式に従ってバイト列をデコードし、自然寸法を返す。
///
/// `path` はエラーメッセージにのみ使い、ファイルシステムは読まない
/// （読み込み済みの `bytes` をそのままデコードする）。形式の判定は
/// [`ImageFormat::from_path`](crate::typeset::ImageFormat) が済ませている。
///
/// # Errors
///
/// デコードに失敗した場合に [`TypesetError`] を返す。
pub(super) fn natural_image_size(path: &str, format: ImageFormat, bytes: &[u8]) -> Result<(f32, f32), TypesetError> {
  return match format {
    ImageFormat::Png => raster_size(path, bytes, image::ImageFormat::Png),
    ImageFormat::Jpeg => raster_size(path, bytes, image::ImageFormat::Jpeg),
    ImageFormat::Svg => svg_size(path, bytes),
  };
}

/// ラスタ画像のピクセル寸法を寸法ヘッダ（PNG は `IHDR`、JPEG は `SOF`）から読む。
///
/// EXIF の Orientation は適用しない — 描画側（krilla）も寸法ヘッダの値をそのまま使うため、
/// 適用すると組版時の自然寸法と描画時の解釈がずれる。
#[expect(clippy::cast_precision_loss, reason = "ピクセル寸法（u32）は f32 の仮数部に収まる")]
fn raster_size(path: &str, bytes: &[u8], format: image::ImageFormat) -> Result<(f32, f32), TypesetError> {
  let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().map_err(|source| {
    return TypesetError::DecodeImage {
      path: path.to_string(),
      source,
    };
  })?;
  return Ok((width as f32, height as f32));
}

/// SVG の width / height を usvg が解釈した値として返す。
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
mod tests {
  use super::natural_image_size;
  use crate::typeset::{error::TypesetError, image::ImageFormat};

  #[test]
  fn natural_image_size_returns_svg_dimensions_from_bytes() {
    // Arrange
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    // Act
    let size = natural_image_size("icon.svg", ImageFormat::Svg, svg).expect("有効な SVG はデコードできるはず");

    // Assert
    assert!((size.0 - 80.0).abs() < 1e-4);
    assert!((size.1 - 60.0).abs() < 1e-4);
  }

  #[test]
  fn natural_image_size_reports_broken_raster_as_decode_error() {
    // Arrange — 形式は PNG と判定済みだが中身が PNG ではない
    let bytes = b"not a png";

    // Act
    let result = natural_image_size("broken.png", ImageFormat::Png, bytes);

    // Assert
    assert!(matches!(result, Err(TypesetError::DecodeImage { path, .. }) if path == "broken.png"));
  }
}
