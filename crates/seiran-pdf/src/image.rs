//! 画像のデコード（PNG / JPEG / SVG / PDF）とラスタ画像のダウンサンプリングを行う。

use std::{io::Cursor, sync::Arc};

use image::ImageReader;
use krilla::{
  Data,
  image::Image,
  pdf::{Pdf, PdfDocument},
};
use seiran_compiler::{ImageFormat, SvgParser};
use usvg::Tree;

use crate::error::PdfRenderError;

/// 読み込んだラスタ画像・SVG・PDF。
pub(crate) enum LoadedImage {
  /// PNG / JPEG などのラスタ画像。
  Raster(Image),
  /// usvg でパースした SVG。
  Svg(Box<Tree>),
  /// 埋め込み用に開いた PDF 文書（描くページは呼び元が選ぶ）。
  Pdf(PdfDocument),
}

/// 判定済みの形式に従ってバイト列をデコードし、必要ならラスタ画像を指定サイズ以下に縮小する。
///
/// `path` はエラーメッセージにのみ使い、ファイルシステムは読まない。`format` は組版段が判定済み。SVG は compiler が
/// 検査したのと同じ設定（`svg`。設定済みフォントで文字を組む）で読む。
pub(crate) fn load_image(
  path: &str,
  format: ImageFormat,
  bytes: &Arc<[u8]>,
  resize_to: Option<(u32, u32)>,
  svg: &SvgParser,
) -> Result<LoadedImage, PdfRenderError> {
  match format {
    ImageFormat::Png => return load_raster(path, bytes, resize_to, image::ImageFormat::Png, Image::from_png),
    ImageFormat::Jpeg => return load_raster(path, bytes, resize_to, image::ImageFormat::Jpeg, Image::from_jpeg),
    ImageFormat::Svg => {
      let tree = svg.parse(bytes).map_err(|source| {
        return PdfRenderError::ParseSvg {
          path: path.to_string(),
          source,
        };
      })?;
      return Ok(LoadedImage::Svg(Box::new(tree)));
    },
    ImageFormat::Pdf => {
      // compiler が同じバイト列を検査済みだが、krilla を compiler へ持ち込まないため描画用にもう 1 度開く。
      // `Arc<[u8]>` を 1 段包むと hayro の `PdfData` が受け取る Sized な `AsRef<[u8]>` になり、複製しない
      let pdf = Pdf::new(Arc::new(Arc::clone(bytes))).map_err(|error| {
        return PdfRenderError::ParsePdf {
          path: path.to_string(),
          reason: format!("{error:?}"),
        };
      })?;
      return Ok(LoadedImage::Pdf(PdfDocument::new(Arc::new(pdf))));
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
///
/// 縮小しないときは読み込み済みのバイト列を複製せず共有したまま渡す。
fn load_raster(
  path: &str,
  bytes: &Arc<[u8]>,
  resize_to: Option<(u32, u32)>,
  reencode_as: image::ImageFormat,
  decode: RasterDecoder,
) -> Result<LoadedImage, PdfRenderError> {
  let data = if let Some(target) = resize_to {
    Data::from(downsample_raster(bytes, target, reencode_as, path)?)
  } else {
    // `Arc<[u8]>` を 1 段包むと krilla の `Data` が受け取る Sized な `AsRef<[u8]>` になり、複製しない
    let shared: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(Arc::clone(bytes));
    Data::from(shared)
  };
  let image = decode(data, INTERPOLATE).map_err(|reason| {
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
  resized.write_to(&mut Cursor::new(&mut out), format).map_err(|source| {
    return PdfRenderError::ResizeImage {
      path: path.to_string(),
      source,
    };
  })?;
  return Ok(out);
}

/// ラスタ画像を `target`（[`required_pixels`] の値）以下へ縮小するなら、その目標ピクセル寸法を返す。
///
/// 寸法ヘッダ（PNG は `IHDR`、JPEG は `SOF`）だけを読み、画素はデコードしない。EXIF の Orientation は
/// 適用しない（krilla も寸法ヘッダの値で描く）。ヘッダを読めないときは `None` を返す — 同じバイト列は続く
/// 未縮小の [`load_image`] で krilla が同じデコーダ（`png` / `zune-jpeg`）で拒否し、そこで診断になる。
pub(crate) fn resize_target(format: ImageFormat, bytes: &[u8], target: (f32, f32)) -> Option<(u32, u32)> {
  let format = match format {
    ImageFormat::Png => image::ImageFormat::Png,
    ImageFormat::Jpeg => image::ImageFormat::Jpeg,
    ImageFormat::Svg | ImageFormat::Pdf => return None,
  };
  let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().ok()?;
  #[expect(
    clippy::cast_precision_loss,
    reason = "f32 で正確なのは 2^24 px までで、それを超える寸法の丸め（相対 2^-24 以下）が判定を変えるのは目標との差がその幅に収まるときだけ"
  )]
  let exceeds = width as f32 > target.0 || height as f32 > target.1;
  if !exceeds {
    return None;
  }
  #[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "`required_pixels` は有限かつ正の値だけを返し、`ceil().max(1.0)` で 1 以上に丸めた後の縮小目標ピクセル数"
  )]
  return Some((target.0.ceil().max(1.0) as u32, target.1.ceil().max(1.0) as u32));
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
  use std::path::Path;

  use seiran_compiler::{MemoryProjectSource, ProjectPath, test_support};

  use super::*;

  /// ラスタ 2 形式の（組版段の判定, `image` crate の再エンコード形式）の対。
  const RASTER_FORMATS: [(ImageFormat, image::ImageFormat); 2] = [
    (ImageFormat::Png, image::ImageFormat::Png),
    (ImageFormat::Jpeg, image::ImageFormat::Jpeg),
  ];

  /// テストフォント 1 本（`vendor/fonts/STIXTwoMath-Regular.ttf`）を全種別に使う最小のプロジェクトを compile し、
  /// その描画資源から SVG パーサを得る。
  fn svg_parser() -> SvgParser {
    let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
    let font_bytes = std::fs::read(&font).expect("テストフォントを読めるはず（tools/fetch-test-assets.sh で取得）");
    let config = format!(
      "sources = [\"main.sei\"]\n\n{}{}{}",
      test_support::valid_output_section("doc", "out"),
      test_support::valid_pdf_section(),
      test_support::font_sections("/project/font.ttf"),
    );
    let source = MemoryProjectSource::new()
      .with_text("/project/config.toml", config)
      .with_text("/project/main.sei", "text")
      .with_bytes("/project/font.ttf", font_bytes);
    let compilation =
      seiran_compiler::compile(&source, &ProjectPath::new(Path::new("/project/config.toml")), Path::new("/project"))
        .expect("最小のプロジェクトは compile できるはず");
    return compilation.publication.resources().svg_parser();
  }

  /// ラスタとして読めたはずの画像のピクセル寸法。
  fn raster_size(loaded: &LoadedImage) -> (u32, u32) {
    let LoadedImage::Raster(image) = loaded else {
      panic!("ラスタ画像を期待");
    };
    return image.size();
  }

  #[test]
  fn load_image_decodes_svg_to_its_declared_size() {
    let svg: Arc<[u8]> = Arc::from(&br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#[..]);

    let loaded =
      load_image("icon.svg", ImageFormat::Svg, &svg, None, &svg_parser()).expect("有効な SVG はデコードできるはず");

    let LoadedImage::Svg(tree) = loaded else {
      panic!("SVG を期待");
    };
    assert!((tree.size().width() - 80.0).abs() < 1e-4);
    assert!((tree.size().height() - 60.0).abs() < 1e-4);
  }

  #[test]
  fn load_image_parses_pdf_into_an_embeddable_document() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image/vector1.pdf");
    let bytes: Arc<[u8]> = std::fs::read(&path).expect("PDF fixture を読めるはず").into();

    let loaded =
      load_image("vector1.pdf", ImageFormat::Pdf, &bytes, None, &svg_parser()).expect("有効な PDF は読めるはず");

    assert!(matches!(loaded, LoadedImage::Pdf(_)));
  }

  #[test]
  fn load_image_reports_broken_pdf_as_parse_error() {
    let result = load_image("broken.pdf", ImageFormat::Pdf, &Arc::from(&b"not a pdf"[..]), None, &svg_parser());

    assert!(matches!(result, Err(PdfRenderError::ParsePdf { path, .. }) if path == "broken.pdf"));
  }

  #[test]
  fn load_image_decodes_raster_by_the_declared_format_not_the_extension() {
    for (format, reencode_as) in RASTER_FORMATS {
      let bytes = raster_bytes(reencode_as, 3, 2);

      let loaded = load_image("figure.bin", format, &bytes, None, &svg_parser())
        .unwrap_or_else(|error| panic!("{format:?} として読めるはず: {error}"));

      assert_eq!(raster_size(&loaded), (3, 2), "{format:?} は 3x2px のまま");
    }
  }

  #[test]
  fn load_image_downsamples_raster_to_fit_the_requested_pixels() {
    // 正方形にして aspect-fit の丸めが結論に効かないようにする
    for (format, reencode_as) in RASTER_FORMATS {
      let bytes = raster_bytes(reencode_as, 4, 4);

      let loaded = load_image("big.img", format, &bytes, Some((2, 2)), &svg_parser())
        .unwrap_or_else(|error| panic!("{format:?} は縮小して読めるはず: {error}"));

      assert_eq!(raster_size(&loaded), (2, 2), "{format:?} は 2x2px に縮むはず");
    }
  }

  #[test]
  fn load_image_reports_unresizable_raster_as_resize_error_before_decoding() {
    for (format, _) in RASTER_FORMATS {
      let result = load_image("broken.img", format, &Arc::from(&b"not an image"[..]), Some((2, 2)), &svg_parser());
      assert!(
        matches!(result, Err(PdfRenderError::ResizeImage { path, .. }) if path == "broken.img"),
        "{format:?} は縮小段階の ResizeImage で止まるはず"
      );
    }
  }

  #[test]
  fn load_image_reports_broken_raster_as_decode_error() {
    for (format, _) in RASTER_FORMATS {
      let result = load_image("broken.img", format, &Arc::from(&b"not an image"[..]), None, &svg_parser());
      assert!(
        matches!(result, Err(PdfRenderError::DecodeImage { path, .. }) if path == "broken.img"),
        "{format:?} は DecodeImage になるはず"
      );
    }
  }

  #[test]
  fn load_image_shares_unresized_raster_bytes_without_copying() {
    // krilla は JPEG を DCTDecode でそのまま埋め込むので、返した `Image` が生きている間は渡した `Data` を保持する。
    // PNG も同じ `load_raster` の分岐を通るが、別スレッドのデコード後に `Data` を手放すので参照数が定まらない
    let bytes = raster_bytes(image::ImageFormat::Jpeg, 3, 2);

    let loaded =
      load_image("figure.jpg", ImageFormat::Jpeg, &bytes, None, &svg_parser()).expect("有効な JPEG は読めるはず");

    assert_eq!(Arc::strong_count(&bytes), 2, "未縮小の JPEG は元のバイト列を共有するはず");
    drop(loaded);
  }

  #[test]
  fn load_image_does_not_retain_source_bytes_when_downsampling() {
    let bytes = raster_bytes(image::ImageFormat::Jpeg, 4, 4);

    let loaded =
      load_image("big.jpg", ImageFormat::Jpeg, &bytes, Some((2, 2)), &svg_parser()).expect("縮小して読めるはず");

    assert_eq!(Arc::strong_count(&bytes), 1, "縮小後の画像は再エンコードしたバイト列だけを持つはず");
    drop(loaded);
  }

  #[test]
  fn resize_target_resizes_only_when_pixels_exceed_the_target() {
    for (format, reencode_as) in RASTER_FORMATS {
      let bytes = raster_bytes(reencode_as, 4, 4);

      assert_eq!(resize_target(format, &bytes, (2.0, 2.0)), Some((2, 2)), "{format:?}");
      assert_eq!(resize_target(format, &bytes, (4.0, 4.0)), None, "{format:?}");
      // 目標が整数でなければ、切り上げた目標が元と同じ寸法でも縮小する（丸める前の目標と比べる）
      assert_eq!(resize_target(format, &bytes, (3.5, 3.5)), Some((4, 4)), "{format:?}");
    }
  }

  #[test]
  fn resize_target_leaves_unreadable_raster_to_the_unresized_load() {
    for (format, _) in RASTER_FORMATS {
      let bytes: Arc<[u8]> = Arc::from(&b"not an image"[..]);

      let resize_to = resize_target(format, &bytes, (2.0, 2.0));
      let result = load_image("broken.img", format, &bytes, resize_to, &svg_parser());

      assert_eq!(resize_to, None, "{format:?}");
      // 縮小段階の ResizeImage ではなく、未縮小の読込で krilla が出す DecodeImage になる
      assert!(
        matches!(result, Err(PdfRenderError::DecodeImage { path, .. }) if path == "broken.img"),
        "{format:?}"
      );
    }
  }

  #[test]
  fn resize_target_reads_only_the_png_header() {
    // 画素データの途中で切った PNG。寸法ヘッダは読めるが、全画素デコードは失敗する
    let bytes = png_cut_inside_idat(4, 4);

    assert_eq!(resize_target(ImageFormat::Png, &bytes, (2.0, 2.0)), Some((2, 2)));
    let result = load_image("cut.png", ImageFormat::Png, &bytes, Some((2, 2)), &svg_parser());
    assert!(matches!(result, Err(PdfRenderError::ResizeImage { path, .. }) if path == "cut.png"));
  }

  /// `width` x `height` の RGB ラスタ画像を `format` でエンコードしたバイト列を返す。
  ///
  /// RGB なのは `image` crate の JPEG エンコーダが RGBA を受け付けないため。
  fn raster_bytes(format: image::ImageFormat, width: u32, height: u32) -> Arc<[u8]> {
    let mut out = Vec::new();
    image::RgbImage::new(width, height)
      .write_to(&mut Cursor::new(&mut out), format)
      .expect("小さな RGB 画像は書き出せるはず");
    return out.into();
  }

  /// `width` x `height` の PNG を、最初の `IDAT` チャンクの画素データ 2 バイト目で切ったバイト列を返す。
  fn png_cut_inside_idat(width: u32, height: u32) -> Arc<[u8]> {
    let bytes = raster_bytes(image::ImageFormat::Png, width, height);
    let idat = bytes
      .windows(4)
      .position(|window| return window == b"IDAT")
      .expect("PNG は IDAT チャンクを持つはず");
    return Arc::from(&bytes[..idat + 4 + 2]);
  }
}
