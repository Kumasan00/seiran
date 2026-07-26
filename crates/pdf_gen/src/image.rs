//! 画像の読み込み、寸法解決、ダウンサンプリングを行う。

use std::{collections::HashMap, fs, path::Path};

use krilla::image::Image;
use model::{AssetId, Block};
use tracing::debug;
use usvg::Tree;

use crate::error::PdfGenError;

/// 画像パスごとの自然寸法と生バイト列。
#[derive(Debug)]
pub struct ImageSet {
  /// パス → 自然寸法（ラスタは px、SVG は usvg が報告した width / height）。
  natural_sizes: HashMap<AssetId, (f32, f32)>,
  /// パス → ファイルから読み込んだ生バイト列（未デコード）。
  bytes: HashMap<AssetId, Vec<u8>>,
}

impl ImageSet {
  /// `path` の自然寸法を返す。`load_image_set` に渡さなかったパスは `None`。
  fn natural_size(&self, path: &AssetId) -> Option<(f32, f32)> { return self.natural_sizes.get(path).copied(); }

  /// 保持していた画像の生バイト列を消費して返す。
  ///
  /// render 用 [`crate::ResourceBundle`] の構築に使う。これを呼んだ後は自然寸法の参照はできない。
  #[must_use]
  pub fn into_image_bytes(self) -> HashMap<AssetId, Vec<u8>> { return self.bytes; }
}

/// 画像ファイルを読み込み、自然寸法と生バイト列を格納した [`ImageSet`] を返す。
///
/// `pdf_gen` 内でファイルシステムを読む唯一の箇所。ここで保持した生バイト列は
/// [`ImageSet::into_image_bytes`] で取り出し、render の入力（`ResourceBundle`）へ渡す。
///
/// # Errors
///
/// 画像の読み込み・デコードに失敗した場合に [`PdfGenError`] を返します。
pub fn load_image_set(paths: &[AssetId]) -> Result<ImageSet, PdfGenError> {
  let mut natural_sizes = HashMap::with_capacity(paths.len());
  let mut bytes_map = HashMap::with_capacity(paths.len());
  for path in paths {
    let file_bytes = fs::read(path.as_str()).map_err(|source| {
      return PdfGenError::ReadImage {
        path: path.as_str().to_string(),
        source,
      };
    })?;
    let loaded = load_image(path.as_str(), &file_bytes, None)?;
    natural_sizes.insert(path.clone(), loaded.natural_size());
    bytes_map.insert(path.clone(), file_bytes);
  }
  debug!(image_count = natural_sizes.len(), "画像の自然寸法を確定しました");
  return Ok(ImageSet {
    natural_sizes,
    bytes: bytes_map,
  });
}

/// ブロック列中の画像サイズを自然寸法と本文幅から確定する。
///
/// # Errors
///
/// 自然寸法が不正な場合、または画像が `images` にない場合に [`PdfGenError`] を返す。
pub fn resolve_images(blocks: Vec<Block>, text_width: f32, images: &ImageSet) -> Result<Vec<Block>, PdfGenError> {
  let resolved = blocks
    .into_iter()
    .map(|block| match block {
      Block::Image {
        path,
        width,
        height,
        target_dpi,
        align,
      } => {
        let (nat_width, nat_height) = images.natural_size(&path).ok_or_else(|| {
          return PdfGenError::ImageNotInManifest { path: path.clone() };
        })?;
        let (final_width, final_height) = resolve_image_size(
          width.map(model::Length::to_pt),
          height.map(model::Length::to_pt),
          nat_width,
          nat_height,
          text_width,
        )
        .ok_or_else(|| {
          return PdfGenError::InvalidImageNaturalSize {
            path: path.clone(),
            width: nat_width,
            height: nat_height,
          };
        })?;
        return Ok(Block::Image {
          path,
          width: Some(model::Length::pt(final_width)),
          height: Some(model::Length::pt(final_height)),
          target_dpi,
          align,
        });
      },
      other => return Ok(other),
    })
    .collect::<Result<Vec<Block>, PdfGenError>>()?;
  let image_count = resolved.iter().filter(|block| matches!(block, Block::Image { .. })).count();
  debug!(image_count, "画像サイズを確定しました");
  return Ok(resolved);
}

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

/// 画像の最終描画寸法を指定値または自然寸法の縦横比から求める。
///
/// 寸法の推論に必要な自然寸法が不正な場合は `None` を返す。
pub(crate) fn resolve_image_size(
  width: Option<f32>,
  height: Option<f32>,
  nat_width: f32,
  nat_height: f32,
  column_width: f32,
) -> Option<(f32, f32)> {
  let aspect_ratio = || {
    if !nat_width.is_finite() || !nat_height.is_finite() || nat_width <= 0.0 || nat_height <= 0.0 {
      return None;
    }
    return Some(nat_height / nat_width);
  };
  match (width, height) {
    (Some(w), Some(h)) => return Some((w, h)),
    (Some(w), None) => {
      let ratio = aspect_ratio()?;
      return Some((w, w * ratio));
    },
    (None, Some(h)) => {
      let ratio = aspect_ratio()?;
      return Some((h / ratio, h));
    },
    (None, None) => {
      let ratio = aspect_ratio()?;
      return Some((column_width, column_width * ratio));
    },
  }
}

/// 画像バイト列をデコードし、自然寸法（ラスタは px、SVG は usvg が報告した width / height）を返す。
///
/// デコードのみを行い、width / height の確定（縦横比・本文幅からの推論）は行わない
/// （compiler 側 `seiran::build_pdf::image_resources::resolve_images` の責務）。
///
/// # Errors
///
/// 画像のデコードに失敗した場合に [`PdfGenError`] を返す。
pub fn natural_image_size(path: &str, bytes: &[u8]) -> Result<(f32, f32), PdfGenError> {
  return load_image(path, bytes, None).map(|loaded| return loaded.natural_size());
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
  fn resolve_image_size_uses_specified_values_when_both_given() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(80.0), Some(60.0), 800.0, 600.0, column_width);

    // Assert
    let (w, h) = resolved.expect("両指定なら必ず Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_infers_height_from_aspect_when_only_width_given() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(80.0), None, 800.0, 600.0, column_width);

    // Assert
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_infers_width_from_aspect_when_only_height_given() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, Some(60.0), 800.0, 600.0, column_width);

    // Assert
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_fits_to_column_when_both_omitted() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, 800.0, 600.0, column_width);

    // Assert
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 400.0).abs() < 1e-4);
    assert!((h - 300.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_returns_none_when_natural_size_zero_and_inference_needed() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, 0.0, 600.0, column_width);

    // Assert
    assert!(resolved.is_none());
  }

  #[test]
  fn resolve_image_size_accepts_fractional_svg_natural_size() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(160.0), None, 320.5, 180.0, column_width);

    // Assert
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    let expected_height = 160.0 * (180.0_f32 / 320.5_f32);
    assert!((w - 160.0).abs() < 1e-4);
    assert!((h - expected_height).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_returns_none_when_natural_size_non_finite() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, f32::NAN, 100.0, column_width);

    // Assert
    assert!(resolved.is_none());
  }

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
  fn natural_image_size_propagates_unsupported_format_error() {
    // Arrange
    let bytes = b"not an image";

    // Act
    let result = natural_image_size("icon.gif", bytes);

    // Assert
    assert!(matches!(result, Err(PdfGenError::UnsupportedImageFormat { path }) if path == "icon.gif"));
  }
}
