//! 画像のデコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプリングを行う。
//!
//! 描画に使う画像本体のデコードだけを担う。自然寸法の解決・width / height の確定は
//! compiler 側 `seiran_compiler` の `typeset::image` に閉じている（epic #276 / #279、#350、#372）。

use krilla::{Data, image::Image};
use seiran_compiler::ImageFormat;
use usvg::Tree;

use crate::error::PdfRenderError;

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
/// `path` はエラーメッセージにのみ使い、ファイルシステムは読まない（読み込み済みの `bytes` を
/// そのままデコードする）。形式は組版段（`seiran_compiler` の `typeset::image`）が判定済みで、
/// ここで拡張子を読み直さない — 同じ判定を 2 回書くと両者が食い違いうるため（#378）。
/// ラスタ 2 形式は [`load_raster`] の 1 経路で読み、アームは再エンコード形式と krilla の
/// コンストラクタを渡すだけになる（#770）。
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
/// 形式ごとに違うのはこの関数値と再エンコード形式の 2 値だけで、縮小・デコード・エラー変換の本体は
/// [`load_raster`] が 1 経路で持つ（#770）。
type RasterDecoder = fn(Data, bool) -> Result<Image, String>;

/// ラスタ画像を必要なら `resize_to` 以下に縮小してから、krilla の `decode` でデコードする。
///
/// PNG / JPEG の差は「縮小後の再エンコード形式 `reencode_as`」と「krilla のコンストラクタ `decode`」の
/// 2 値だけで、縮小 → デコード → [`PdfRenderError::DecodeImage`] への変換の本体はここ 1 箇所が持つ。
/// SVG はこの経路を通らない（[`load_image`] の SVG アームが `usvg` で直接パースする）。
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
  let image = decode(bytes.into(), false).map_err(|reason| {
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
  ///
  /// 「PNG と JPEG が同じ経路を通る」ことを固定するテストはこの表を回す（#770）。
  const RASTER_FORMATS: [(ImageFormat, image::ImageFormat); 2] = [
    (ImageFormat::Png, image::ImageFormat::Png),
    (ImageFormat::Jpeg, image::ImageFormat::Jpeg),
  ];

  #[test]
  fn load_image_decodes_svg_to_its_declared_size() {
    // Arrange
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    // Act
    let loaded = load_image("icon.svg", ImageFormat::Svg, svg, None).expect("有効な SVG はデコードできるはず");

    // Assert
    let (width, height) = loaded.natural_size();
    assert!((width - 80.0).abs() < 1e-4);
    assert!((height - 60.0).abs() < 1e-4);
  }

  #[test]
  fn load_image_dispatches_on_the_declared_format_not_the_extension() {
    // Arrange — 拡張子は .bin だが組版段が PNG と判定した実 PNG バイト列
    let png = raster_bytes(image::ImageFormat::Png, 1, 1);

    // Act
    let loaded = load_image("figure.bin", ImageFormat::Png, &png, None).expect("PNG として読めるはず");

    // Assert
    let (width, height) = loaded.natural_size();
    assert!((width - 1.0).abs() < 1e-4);
    assert!((height - 1.0).abs() < 1e-4);
  }

  #[test]
  fn load_image_decodes_jpeg_through_the_same_raster_path() {
    // #770: JPEG も PNG と同じ経路で読み、自然寸法は krilla が報告するピクセル数になる
    // （変更前後で同じ結果になる固定テスト。赤 → 緑の TDD ではない）
    // Arrange
    let jpeg = raster_bytes(image::ImageFormat::Jpeg, 3, 2);

    // Act
    let loaded = load_image("photo.jpg", ImageFormat::Jpeg, &jpeg, None).expect("JPEG として読めるはず");

    // Assert
    let (width, height) = loaded.natural_size();
    assert!((width - 3.0).abs() < 1e-4);
    assert!((height - 2.0).abs() < 1e-4);
  }

  #[test]
  fn load_image_downsamples_raster_to_fit_the_requested_pixels() {
    // #770: `resize_to` は PNG / JPEG のどちらでも同じ規則（縦横比を保って収める）で効く
    // （変更前後で同じ結果になる固定テスト）。正方形にして aspect-fit の丸めが結論に効かないようにする
    for (format, reencode_as) in RASTER_FORMATS {
      // Arrange
      let bytes = raster_bytes(reencode_as, 4, 4);

      // Act
      let loaded = load_image("big.img", format, &bytes, Some((2, 2))).expect("縮小して読めるはず");

      // Assert
      let (width, height) = loaded.natural_size();
      assert!((width - 2.0).abs() < 1e-4, "{format:?} の幅は 2px に縮むはず");
      assert!((height - 2.0).abs() < 1e-4, "{format:?} の高さは 2px に縮むはず");
    }
  }

  #[test]
  fn load_image_reports_unresizable_raster_as_resize_error_before_decoding() {
    // #770: 縮小はデコードより先に走る。壊れたバイト列 + `resize_to` は image crate 側の ResizeImage で
    // 止まり、krilla の DecodeImage には届かない（順序を固定するテスト。変更前後で同じ）
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
    // 形式は判定済みだが中身がその形式ではない — PNG / JPEG とも krilla のデコード失敗が DecodeImage になる
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
  /// RGB なのは `image` crate の JPEG エンコーダが RGBA を受け付けないため（PNG はどちらでも書ける）。
  fn raster_bytes(format: image::ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbImage::new(width, height)
      .write_to(&mut std::io::Cursor::new(&mut out), format)
      .expect("小さな RGB 画像は書き出せるはず");
    return out;
  }
}
