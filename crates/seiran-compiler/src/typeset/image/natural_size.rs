//! 画像バイト列から自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）を得る。
//!
//! デコードと寸法の検証だけを行い、表示寸法の確定（縦横比・段幅からの推論）は行わない
//! （兄弟 module `resources` の `resolve_image_size` の責務）。ラスタは寸法ヘッダだけを読み、
//! 描画に使う画像本体のデコードは render（`seiran-pdf`）が別に行う。

use std::io::Cursor;

use image::ImageReader;

use crate::{publication::ImageFormat, typeset::error::TypesetError};

/// 検証済みの自然寸法（幅・高さとも有限かつ正）。
///
/// デコーダ（`image` / `usvg`）が報告した値をこの型へ通す時点で検証するので、表示寸法の確定
/// （`super::resources::resolve_image_size`）は縦横比を必ず算出でき、失敗しない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct NaturalSize {
  /// 自然幅（ラスタはピクセル、SVG は usvg が報告した width）
  width: f32,
  /// 自然高さ（ラスタはピクセル、SVG は usvg が報告した height）
  height: f32,
}

impl NaturalSize {
  /// 有限かつ正の幅・高さだけを通す。縦横比を算出できない値は `None`。
  pub(super) fn new(width: f32, height: f32) -> Option<Self> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
      return None;
    }
    return Some(NaturalSize { width, height });
  }

  /// 自然幅（テスト専用アクセサ。本体は `aspect_ratio` 経由で縦横比だけを使う）
  #[cfg(test)]
  pub(super) fn width(self) -> f32 { return self.width; }

  /// 自然高さ（テスト専用アクセサ。本体は `aspect_ratio` 経由で縦横比だけを使う）
  #[cfg(test)]
  pub(super) fn height(self) -> f32 { return self.height; }

  /// 縦横比（高さ / 幅）。コンストラクタが正の有限値だけを通すので必ず有限。
  pub(super) fn aspect_ratio(self) -> f32 { return self.height / self.width; }
}

/// 判定済みの画像形式に従ってバイト列をデコードし、検証済みの自然寸法を返す。
///
/// `path` はエラーメッセージにのみ使い、ファイルシステムは読まない
/// （読み込み済みの `bytes` をそのままデコードする）。形式の判定は
/// [`ImageFormat::from_path`](crate::publication::ImageFormat) が済ませている。
/// デコーダが報告した寸法をここで検証するので、下流は縦横比を算出できる値しか受け取らない。
///
/// # Errors
///
/// デコードに失敗した場合と、報告された寸法が有限かつ正でない場合に [`TypesetError`] を返す。
pub(super) fn natural_image_size(path: &str, format: ImageFormat, bytes: &[u8]) -> Result<NaturalSize, TypesetError> {
  let (width, height) = match format {
    ImageFormat::Png => raster_size(path, bytes, image::ImageFormat::Png),
    ImageFormat::Jpeg => raster_size(path, bytes, image::ImageFormat::Jpeg),
    ImageFormat::Svg => svg_size(path, bytes),
  }?;
  return NaturalSize::new(width, height).ok_or_else(|| {
    return TypesetError::InvalidImageNaturalSize {
      path: path.to_string(),
      width,
      height,
    };
  });
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
  use super::{NaturalSize, natural_image_size};
  use crate::{publication::ImageFormat, typeset::error::TypesetError};

  #[test]
  fn natural_image_size_returns_svg_dimensions_from_bytes() {
    // Arrange
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    // Act
    let size = natural_image_size("icon.svg", ImageFormat::Svg, svg).expect("有効な SVG はデコードできるはず");

    // Assert
    assert!((size.width() - 80.0).abs() < 1e-4);
    assert!((size.height() - 60.0).abs() < 1e-4);
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

  #[test]
  fn natural_size_rejects_zero_and_non_finite_values() {
    assert!(NaturalSize::new(0.0, 60.0).is_none(), "幅 0 は縦横比を算出できないので弾くはず");
    assert!(NaturalSize::new(80.0, 0.0).is_none(), "高さ 0 は縦横比を算出できないので弾くはず");
    assert!(NaturalSize::new(-80.0, 60.0).is_none(), "負の幅は弾くはず");
    assert!(NaturalSize::new(f32::NAN, 60.0).is_none(), "NaN は弾くはず");
    assert!(NaturalSize::new(f32::INFINITY, 60.0).is_none(), "無限大は弾くはず");
  }

  #[test]
  fn natural_size_keeps_positive_finite_values_and_aspect_ratio() {
    // Arrange / Act
    let size = NaturalSize::new(320.0, 240.0).expect("正の有限値は通るはず");

    // Assert
    assert!((size.width() - 320.0).abs() < 1e-4);
    assert!((size.height() - 240.0).abs() < 1e-4);
    assert!((size.aspect_ratio() - 0.75).abs() < 1e-4, "縦横比は高さ / 幅");
  }
}
