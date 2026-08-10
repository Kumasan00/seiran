//! 画像のデコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプリングを行う。
//!
//! 描画に使う画像本体のデコードだけを担う。自然寸法の解決・width / height の確定は
//! compiler 側 `seiran_compiler` の `typeset::image` に閉じている（epic #276 / #279、#350、#372）。

use std::path::Path;

use krilla::image::Image;
use usvg::Tree;

use crate::error::PdfGenError;

/// 読み込んだラスタ画像または SVG。
pub(crate) enum LoadedImage {
  /// PNG / JPEG などのラスタ画像。
  Raster(Image),
  /// usvg でパースした SVG。
  ///
  /// バリアント間のサイズ差を抑えるためヒープに保持する。
  Svg(Box<Tree>),
}

impl LoadedImage {
  /// 自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）を返します。
  #[allow(clippy::cast_precision_loss)]
  pub(crate) fn natural_size(&self) -> (f32, f32) {
    return match self {
      LoadedImage::Raster(image) => {
        let (w, h) = image.size();
        (w as f32, h as f32)
      },
      LoadedImage::Svg(tree) => {
        let size = tree.size();
        (size.width(), size.height())
      },
    };
  }
}

/// PNG、JPEG、SVG のバイト列をデコードし、必要ならラスタ画像を指定サイズ以下に縮小する。
///
/// `path` は拡張子判定とエラーメッセージにのみ使い、ファイルシステムは読まない
/// （読み込み済みの `bytes` をそのままデコードする）。
pub(crate) fn load_image(path: &str, bytes: &[u8], resize_to: Option<(u32, u32)>) -> Result<LoadedImage, PdfGenError> {
  let extension = Path::new(path)
    .extension()
    .and_then(|e| return e.to_str())
    .map(str::to_ascii_lowercase)
    .unwrap_or_default();
  match extension.as_str() {
    "png" => {
      let bytes: Vec<u8> = if let Some(target) = resize_to {
        downsample_raster(bytes, target, image::ImageFormat::Png, path)?
      } else {
        bytes.to_vec()
      };
      let image = Image::from_png(bytes.into(), false).map_err(|reason| {
        return PdfGenError::DecodeImage {
          path: path.to_string(),
          reason,
        };
      })?;
      return Ok(LoadedImage::Raster(image));
    },
    "jpg" | "jpeg" => {
      let bytes: Vec<u8> = if let Some(target) = resize_to {
        downsample_raster(bytes, target, image::ImageFormat::Jpeg, path)?
      } else {
        bytes.to_vec()
      };
      let image = Image::from_jpeg(bytes.into(), false).map_err(|reason| {
        return PdfGenError::DecodeImage {
          path: path.to_string(),
          reason,
        };
      })?;
      return Ok(LoadedImage::Raster(image));
    },
    "svg" => {
      let tree = Tree::from_data(bytes, &usvg::Options::default()).map_err(|source| {
        return PdfGenError::ParseSvg {
          path: path.to_string(),
          source,
        };
      })?;
      return Ok(LoadedImage::Svg(Box::new(tree)));
    },
    _ => {
      return Err(PdfGenError::UnsupportedImageFormat {
        path: path.to_string(),
      });
    },
  }
}

/// ラスタ画像を指定ピクセル寸法以下に縮小し、同じ形式で再エンコードする。
fn downsample_raster(
  bytes: &[u8],
  target_px: (u32, u32),
  format: image::ImageFormat,
  path: &str,
) -> Result<Vec<u8>, PdfGenError> {
  let img = image::load_from_memory_with_format(bytes, format).map_err(|source| {
    return PdfGenError::ResizeImage {
      path: path.to_string(),
      source,
    };
  })?;
  let resized = img.resize(target_px.0, target_px.1, image::imageops::FilterType::Lanczos3);
  let mut out: Vec<u8> = Vec::new();
  resized.write_to(&mut std::io::Cursor::new(&mut out), format).map_err(|source| {
    return PdfGenError::ResizeImage {
      path: path.to_string(),
      source,
    };
  })?;
  return Ok(out);
}

/// 描画寸法と上限 DPI から必要なピクセル寸法を求める。
pub(crate) fn required_pixels(width_pt: f32, height_pt: f32, dpi: u32) -> Option<(f32, f32)> {
  if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 || dpi == 0 {
    return None;
  }
  #[allow(clippy::cast_precision_loss)]
  let dpi_f = dpi as f32;
  return Some((width_pt / 72.0 * dpi_f, height_pt / 72.0 * dpi_f));
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn load_image_decodes_svg_to_its_declared_size() {
    // Arrange
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    // Act
    let loaded = load_image("icon.svg", svg, None).expect("有効な SVG はデコードできるはず");

    // Assert
    let (width, height) = loaded.natural_size();
    assert!((width - 80.0).abs() < 1e-4);
    assert!((height - 60.0).abs() < 1e-4);
  }

  #[test]
  fn load_image_rejects_unsupported_extension() {
    // Arrange
    let bytes = b"not an image";

    // Act
    let result = load_image("icon.gif", bytes, None);

    // Assert
    assert!(matches!(result, Err(PdfGenError::UnsupportedImageFormat { path }) if path == "icon.gif"));
  }
}
