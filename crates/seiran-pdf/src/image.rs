//! 画像のデコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプリングを行う。

use krilla::{Data, image::Image};
use seiran_compiler::ImageFormat;
use usvg::Tree;

use crate::error::PdfRenderError;

/// 読み込んだラスタ画像または SVG。
pub(crate) enum LoadedImage {
  /// PNG / JPEG などのラスタ画像。
  Raster(Image),
  /// usvg でパースした SVG。
  Svg(Box<Tree>),
}

impl LoadedImage {
  /// 自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）を返します。
  #[expect(
    clippy::cast_precision_loss,
    reason = "ラスタの自然寸法は image crate が返すピクセル数で、f32 の仮数部に収まる"
  )]
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

/// 判定済みの形式に従ってバイト列をデコードし、必要ならラスタ画像を指定サイズ以下に縮小する。
///
/// `path` はエラーメッセージにのみ使い、ファイルシステムは読まない。`format` は組版段が判定済み。
pub(crate) fn load_image(
  path: &str,
  format: ImageFormat,
  bytes: &[u8],
  resize_to: Option<(u32, u32)>,
) -> Result<LoadedImage, PdfRenderError> {
  match format {
    ImageFormat::Png => return load_raster(path, bytes, resize_to, image::ImageFormat::Png, Image::from_png),
    ImageFormat::Jpeg => return load_raster(path, bytes, resize_to, image::ImageFormat::Jpeg, Image::from_jpeg),
    ImageFormat::Svg => {
      let tree = Tree::from_data(bytes, &usvg::Options::default()).map_err(|source| {
        return PdfRenderError::ParseSvg {
          path: path.to_string(),
          source,
        };
      })?;
      return Ok(LoadedImage::Svg(Box::new(tree)));
    },
  }
}

/// krilla のラスタ画像コンストラクタの共通の形（`Image::from_png` / `Image::from_jpeg`）。
///
/// 第 1 引数はエンコード済みのバイト列、第 2 引数は krilla の `interpolate`（[`INTERPOLATE`] で固定）。
type RasterDecoder = fn(Data, bool) -> Result<Image, String>;

/// krilla のコンストラクタへ渡す `interpolate`。画像 `XObject` の `/Interpolate`（ビューアが拡大時に
/// ピクセルを補間するかの指示）で、ラスタ画像には指示しない。
const INTERPOLATE: bool = false;

/// ラスタ画像を必要なら `resize_to` 以下に縮小してから、krilla の `decode` でデコードする。
fn load_raster(
  path: &str,
  bytes: &[u8],
  resize_to: Option<(u32, u32)>,
  reencode_as: image::ImageFormat,
  decode: RasterDecoder,
) -> Result<LoadedImage, PdfRenderError> {
  let bytes: Vec<u8> = if let Some(target) = resize_to {
    downsample_raster(bytes, target, reencode_as, path)?
  } else {
    bytes.to_vec()
  };
  let image = decode(bytes.into(), INTERPOLATE).map_err(|reason| {
    return PdfRenderError::DecodeImage {
      path: path.to_string(),
      reason,
    };
  })?;
  return Ok(LoadedImage::Raster(image));
}

/// ラスタ画像を指定ピクセル寸法以下に縮小し、同じ形式で再エンコードする。
fn downsample_raster(
  bytes: &[u8],
  target_px: (u32, u32),
  format: image::ImageFormat,
  path: &str,
) -> Result<Vec<u8>, PdfRenderError> {
  let img = image::load_from_memory_with_format(bytes, format).map_err(|source| {
    return PdfRenderError::ResizeImage {
      path: path.to_string(),
      source,
    };
  })?;
  let resized = img.resize(target_px.0, target_px.1, image::imageops::FilterType::Lanczos3);
  let mut out: Vec<u8> = Vec::new();
  resized.write_to(&mut std::io::Cursor::new(&mut out), format).map_err(|source| {
    return PdfRenderError::ResizeImage {
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
  #[expect(
    clippy::cast_precision_loss,
    reason = "dpi は `[image]` の上限 DPI で、f32 の仮数部を超える桁にはならない"
  )]
  let dpi_f = dpi as f32;
  return Some((width_pt / 72.0 * dpi_f, height_pt / 72.0 * dpi_f));
}

#[cfg(test)]
mod tests {
  use super::*;

  /// ラスタ 2 形式の（組版段の判定, `image` crate の再エンコード形式）の対。
  const RASTER_FORMATS: [(ImageFormat, image::ImageFormat); 2] = [
    (ImageFormat::Png, image::ImageFormat::Png),
    (ImageFormat::Jpeg, image::ImageFormat::Jpeg),
  ];

  #[test]
  fn load_image_decodes_svg_to_its_declared_size() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    let loaded = load_image("icon.svg", ImageFormat::Svg, svg, None).expect("有効な SVG はデコードできるはず");

    let (width, height) = loaded.natural_size();
    assert!((width - 80.0).abs() < 1e-4);
    assert!((height - 60.0).abs() < 1e-4);
  }

  #[test]
  fn load_image_decodes_raster_by_the_declared_format_not_the_extension() {
    for (format, reencode_as) in RASTER_FORMATS {
      let bytes = raster_bytes(reencode_as, 3, 2);

      let loaded = load_image("figure.bin", format, &bytes, None)
        .unwrap_or_else(|error| panic!("{format:?} として読めるはず: {error}"));

      let (width, height) = loaded.natural_size();
      assert!((width - 3.0).abs() < 1e-4, "{format:?} の幅は 3px のまま");
      assert!((height - 2.0).abs() < 1e-4, "{format:?} の高さは 2px のまま");
    }
  }

  #[test]
  fn load_image_downsamples_raster_to_fit_the_requested_pixels() {
    // 正方形にして aspect-fit の丸めが結論に効かないようにする
    for (format, reencode_as) in RASTER_FORMATS {
      let bytes = raster_bytes(reencode_as, 4, 4);

      let loaded = load_image("big.img", format, &bytes, Some((2, 2)))
        .unwrap_or_else(|error| panic!("{format:?} は縮小して読めるはず: {error}"));

      let (width, height) = loaded.natural_size();
      assert!((width - 2.0).abs() < 1e-4, "{format:?} の幅は 2px に縮むはず");
      assert!((height - 2.0).abs() < 1e-4, "{format:?} の高さは 2px に縮むはず");
    }
  }

  #[test]
  fn load_image_reports_unresizable_raster_as_resize_error_before_decoding() {
    for (format, _) in RASTER_FORMATS {
      let result = load_image("broken.img", format, b"not an image", Some((2, 2)));
      assert!(
        matches!(result, Err(PdfRenderError::ResizeImage { path, .. }) if path == "broken.img"),
        "{format:?} は縮小段階の ResizeImage で止まるはず"
      );
    }
  }

  #[test]
  fn load_image_reports_broken_raster_as_decode_error() {
    for (format, _) in RASTER_FORMATS {
      let result = load_image("broken.img", format, b"not an image", None);
      assert!(
        matches!(result, Err(PdfRenderError::DecodeImage { path, .. }) if path == "broken.img"),
        "{format:?} は DecodeImage になるはず"
      );
    }
  }

  /// `width` x `height` の RGB ラスタ画像を `format` でエンコードしたバイト列を返す。
  ///
  /// RGB なのは `image` crate の JPEG エンコーダが RGBA を受け付けないため。
  fn raster_bytes(format: image::ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbImage::new(width, height)
      .write_to(&mut std::io::Cursor::new(&mut out), format)
      .expect("小さな RGB 画像は書き出せるはず");
    return out;
  }
}
