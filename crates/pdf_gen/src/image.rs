//! 画像のロード・自然寸法解決・ダウンサンプリング。
//!
//! `\image` コマンドが指定するパスを読み込み、PNG / JPEG / SVG を統一的に扱う
//! [`LoadedImage`] に変換する。最終描画寸法（pt）の算出と、ラスタ画像の
//! 必要ピクセル数の見積もりもこのモジュールが受け持つ。

use std::{fs, path::Path};

use hlist::Block;
use krilla::image::Image;
use usvg::Tree;

use crate::error::PdfGenError;

/// ブロック列中の画像サイズを確定する prepass
///
/// `Block::Image` の `width` / `height` が未指定（`None`）の場合に画像ファイルを開いて
/// 自然寸法を取得し、縦横比と本文幅から最終物理サイズ（pt）を確定する。
/// 縦組版（`hlist::break_pages`）が画像高さで改ページ判定できるよう、
/// (a) `build_blocks` と (c+d) `break_pages` の間に挟む。
///
/// # Errors
///
/// 画像の読み込み・デコードに失敗した場合、または自然寸法から縦横比を
/// 算出できない場合に [`PdfGenError`] を返します。
pub fn resolve_images(blocks: Vec<Block>, text_width: f32) -> Result<Vec<Block>, PdfGenError> {
  return blocks
    .into_iter()
    .map(|block| match block {
      Block::Image {
        path,
        width,
        height,
        target_dpi,
        align,
      } => {
        let loaded = load_image(&path, None)?;
        let (nat_width, nat_height) = loaded.natural_size();
        let (final_width, final_height) = resolve_image_size(width, height, nat_width, nat_height, text_width)
          .ok_or_else(|| PdfGenError::InvalidImageNaturalSize {
            path: path.clone(),
            width: nat_width,
            height: nat_height,
          })?;
        return Ok(Block::Image {
          path,
          width: Some(final_width),
          height: Some(final_height),
          target_dpi,
          align,
        });
      },
      other => Ok(other),
    })
    .collect();
}

/// `load_image` が返す画像表現。
///
/// ラスタ画像は krilla の [`Image`] として、SVG は usvg の [`Tree`] として保持し、
/// レンダリング時に呼び出し側が分岐して描画します。
pub(crate) enum LoadedImage {
  /// PNG / JPEG などのラスタ画像。
  Raster(Image),
  /// SVG を usvg でパースした結果。
  ///
  /// `usvg::Tree` は数百バイト規模のため、列挙の他バリアントとのサイズ差を抑える目的で
  /// ヒープに退避します（`clippy::large_enum_variant` 対策）。
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

/// 画像の最終描画寸法（pt）を解決します。
///
/// `\image` の `width` / `height` 任意引数は両方とも省略可能で、未指定分は元画像の
/// 自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）の縦横比から
/// 自動算出します。両方とも省略された場合は本文幅にフィットさせ、高さは縦横比から
/// 算出します（Typst 方式）。
///
/// 戻り値が `None` になるのは、`width` / `height` のどちらかを算出する必要があるにも
/// かかわらず自然寸法が 0 以下または非有限値で縦横比を取れないケースです。
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

/// 画像ファイルを読み込み、拡張子に応じて [`LoadedImage`] に変換します。
///
/// 対応形式は PNG（`.png`）, JPEG（`.jpg` / `.jpeg`）, SVG（`.svg`）です。
/// それ以外の拡張子は [`PdfGenError::UnsupportedImageFormat`] を返します。
///
/// `resize_to` が `Some((w_px, h_px))` のときはラスタ画像をデコード → Lanczos3 リサイズ →
/// 同一フォーマットで再エンコードしてから krilla に渡します。SVG は無視されます
/// （ベクタなので再ラスタライズは不要）。フォーマット変換は行わず、入力が PNG なら PNG、
/// JPEG なら JPEG のまま出力します。
pub(crate) fn load_image(path: &str, resize_to: Option<(u32, u32)>) -> Result<LoadedImage, PdfGenError> {
  let bytes = fs::read(path).map_err(|source| PdfGenError::ReadImage {
    path: path.to_string(),
    source,
  })?;
  let extension = Path::new(path)
    .extension()
    .and_then(|e| e.to_str())
    .map(str::to_ascii_lowercase)
    .unwrap_or_default();
  match extension.as_str() {
    "png" => {
      let bytes = if let Some(target) = resize_to {
        downsample_raster(&bytes, target, image::ImageFormat::Png, path)?
      } else {
        bytes
      };
      let image = Image::from_png(bytes.into(), false).map_err(|reason| PdfGenError::DecodeImage {
        path: path.to_string(),
        reason,
      })?;
      return Ok(LoadedImage::Raster(image));
    },
    "jpg" | "jpeg" => {
      let bytes = if let Some(target) = resize_to {
        downsample_raster(&bytes, target, image::ImageFormat::Jpeg, path)?
      } else {
        bytes
      };
      let image = Image::from_jpeg(bytes.into(), false).map_err(|reason| PdfGenError::DecodeImage {
        path: path.to_string(),
        reason,
      })?;
      return Ok(LoadedImage::Raster(image));
    },
    "svg" => {
      let tree = Tree::from_data(&bytes, &usvg::Options::default()).map_err(|source| PdfGenError::ParseSvg {
        path: path.to_string(),
        source,
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

/// ラスタ画像を `target_px = (w_px, h_px)` 以下に縮小して同一フォーマットで再エンコードする。
///
/// `image::load_from_memory_with_format` でデコードし、`Lanczos3` フィルタで縮小、
/// `format` で再エンコードしたバイト列を返す。JPEG 品質は image クレートの既定値
/// （現状 75）に従う。フォーマットを跨いだ変換（PNG → JPEG 等）は行わない。
fn downsample_raster(
  bytes: &[u8],
  target_px: (u32, u32),
  format: image::ImageFormat,
  path: &str,
) -> Result<Vec<u8>, PdfGenError> {
  let img = image::load_from_memory_with_format(bytes, format).map_err(|source| PdfGenError::ResizeImage {
    path: path.to_string(),
    source,
  })?;
  let resized = img.resize(target_px.0, target_px.1, image::imageops::FilterType::Lanczos3);
  let mut out: Vec<u8> = Vec::new();
  resized
    .write_to(&mut std::io::Cursor::new(&mut out), format)
    .map_err(|source| PdfGenError::ResizeImage {
      path: path.to_string(),
      source,
    })?;
  return Ok(out);
}

/// 描画寸法（pt）と上限 DPI から必要なピクセル寸法を算出する。
///
/// 1 pt = 1/72 inch なので `px = pt / 72 * dpi`。負値や非有限値、ゼロは `None` を返す。
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

    // Assert — 両指定は自然寸法に依存せずそのまま返る
    let (w, h) = resolved.expect("両指定なら必ず Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_infers_height_from_aspect_when_only_width_given() {
    // Arrange — 4:3 の画像で width のみ指定
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(80.0), None, 800.0, 600.0, column_width);

    // Assert — height = width * (nat_h / nat_w) = 80 * (600 / 800) = 60
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_infers_width_from_aspect_when_only_height_given() {
    // Arrange — 4:3 の画像で height のみ指定
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, Some(60.0), 800.0, 600.0, column_width);

    // Assert — width = height * (nat_w / nat_h) = 60 * (800 / 600) = 80
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_fits_to_column_when_both_omitted() {
    // Arrange — 4:3 の画像でサイズ全省略
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, 800.0, 600.0, column_width);

    // Assert — width=column_width, height は縦横比から
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 400.0).abs() < 1e-4);
    assert!((h - 300.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_returns_none_when_natural_size_zero_and_inference_needed() {
    // Arrange — 自然寸法が 0 だと縦横比が出せない
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, 0.0, 600.0, column_width);

    // Assert — None を返してエラーへ
    assert!(resolved.is_none());
  }

  #[test]
  fn resolve_image_size_accepts_fractional_svg_natural_size() {
    // Arrange — SVG の自然寸法は f32 で来る。縦横比 16:9 のベクタ画像で width のみ指定
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(160.0), None, 320.5, 180.0, column_width);

    // Assert — height = 160 * (180 / 320.5)
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    let expected_height = 160.0 * (180.0_f32 / 320.5_f32);
    assert!((w - 160.0).abs() < 1e-4);
    assert!((h - expected_height).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_returns_none_when_natural_size_non_finite() {
    // Arrange — NaN / 無限大は安全側に倒して None
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, f32::NAN, 100.0, column_width);

    // Assert
    assert!(resolved.is_none());
  }
}
