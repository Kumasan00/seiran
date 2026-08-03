//! 画像の自然寸法解決とブロック列への表示寸法確定（旧 `pdf_gen::image`、epic #276 / #279 で移設）

use std::collections::HashMap;

use model::{AssetId, Length};
use tracing::debug;
use typeset::Block;

use super::error::{BuildPdfError, CompilerBug};

/// 画像パスごとの自然寸法と生バイト列（旧 `pdf_gen::ImageSet`）。
#[derive(Debug)]
pub(super) struct ImageResources {
  /// パス → 自然寸法（ラスタは px、SVG は usvg が報告した width / height）。
  natural_sizes: HashMap<AssetId, (f32, f32)>,
  /// パス → ファイルから読み込んだ生バイト列（未デコード）。
  bytes: HashMap<AssetId, Vec<u8>>,
}

impl ImageResources {
  /// `path` の自然寸法を返す。`load_image_resources` に渡さなかったパスは `None`。
  fn natural_size(&self, path: &AssetId) -> Option<(f32, f32)> { return self.natural_sizes.get(path).copied(); }

  /// 保持していた画像の生バイト列を消費して返す。
  ///
  /// render 用 `pdf_gen::ResourceBundle` の構築に使う。これを呼んだ後は自然寸法の参照はできない。
  #[must_use]
  pub(super) fn into_image_bytes(self) -> HashMap<AssetId, Vec<u8>> { return self.bytes; }
}

/// 画像ファイルを読み込み、自然寸法と生バイト列を格納した [`ImageResources`] を返す。
///
/// 画像ファイルを読む唯一の箇所。`source`（[`config::ProjectSource`]）経由で読み込むため、
/// 本体コードはここでも `std::fs` に直接触れない。ここで保持した生バイト列は
/// [`ImageResources::into_image_bytes`] で取り出し、render の入力（`ResourceBundle`）へ渡す。
///
/// # Errors
///
/// 画像の読み込み・デコードに失敗した場合に [`BuildPdfError`] を返す。
#[allow(clippy::result_large_err)]
pub(super) fn load_image_resources(
  source: &dyn config::ProjectSource,
  paths: &[AssetId],
) -> Result<ImageResources, BuildPdfError> {
  let mut natural_sizes = HashMap::with_capacity(paths.len());
  let mut bytes_map = HashMap::with_capacity(paths.len());
  for path in paths {
    let file_bytes = source.read_bytes(&config::ProjectPath::new(path.as_str())).map_err(|source| {
      return BuildPdfError::ReadImage {
        path: path.as_str().to_string(),
        source: source.into_io(),
      };
    })?;
    let natural_size = pdf_gen::natural_image_size(path.as_str(), &file_bytes).map_err(|source| {
      return BuildPdfError::LoadImage {
        path: path.as_str().to_string(),
        source,
      };
    })?;
    natural_sizes.insert(path.clone(), natural_size);
    bytes_map.insert(path.clone(), file_bytes.to_vec());
  }
  debug!(image_count = natural_sizes.len(), "画像の自然寸法を確定しました");
  return Ok(ImageResources {
    natural_sizes,
    bytes: bytes_map,
  });
}

/// ブロック列中の画像サイズを自然寸法と本文幅から確定する。
///
/// # Errors
///
/// 自然寸法が不正な場合、または画像が `images` にない場合に [`BuildPdfError`] を返す。
#[allow(clippy::result_large_err)]
pub(super) fn resolve_images(
  blocks: Vec<Block>,
  text_width: f32,
  images: &ImageResources,
) -> Result<Vec<Block>, BuildPdfError> {
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
          return BuildPdfError::Bug(CompilerBug::new(format!(
            "ImageResources に存在しない画像パスです: {path}（ImageManifest の収集ロジックに不具合があります）"
          )));
        })?;
        let (final_width, final_height) =
          resolve_image_size(width.map(Length::to_pt), height.map(Length::to_pt), nat_width, nat_height, text_width)
            .ok_or_else(|| {
              return BuildPdfError::InvalidImageNaturalSize {
                path: path.clone(),
                width: nat_width,
                height: nat_height,
              };
            })?;
        return Ok(Block::Image {
          path,
          width: Some(Length::pt(final_width)),
          height: Some(Length::pt(final_height)),
          target_dpi,
          align,
        });
      },
      other => return Ok(other),
    })
    .collect::<Result<Vec<Block>, BuildPdfError>>()?;
  let image_count = resolved.iter().filter(|block| matches!(block, Block::Image { .. })).count();
  debug!(image_count, "画像サイズを確定しました");
  return Ok(resolved);
}

/// 画像の最終描画寸法を指定値または自然寸法の縦横比から求める。
///
/// 寸法の推論に必要な自然寸法が不正な場合は `None` を返す。
fn resolve_image_size(
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

#[cfg(test)]
mod tests {
  use std::path::Path;

  use config::MemoryProjectSource;

  use super::*;

  /// リポジトリ直下の `tests/image/` にある実 fixture を `CARGO_MANIFEST_DIR` 基準で読む。
  ///
  /// `crates/seiran` から見て 2 階層上がワークスペースルート（`golden.rs::workspace_root` と同じ関係）。
  fn read_image_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image").join(name);
    return std::fs::read(&path).unwrap_or_else(|error| panic!("画像 fixture を読めるはず: {path:?}: {error}"));
  }

  #[test]
  fn load_image_resources_reads_through_project_source() {
    // Arrange — 実 fixture（tests/image/testimage5.png、3024x4032 の PNG）のバイト列を
    // MemoryProjectSource に登録する。`with_bytes` に渡す前に長さを控え、後で
    // 「登録したバイト列がそのまま保持されているか」を検証できるようにする
    let png_bytes = read_image_fixture("testimage5.png");
    let expected_len = png_bytes.len();
    let source = MemoryProjectSource::new().with_bytes("/project/testimage5.png", png_bytes);
    let paths = vec![AssetId::new("/project/testimage5.png")];

    // Act
    let resources = load_image_resources(&source, &paths).expect("メモリ上の fixture を読めるはず");

    // Assert — 自然寸法（fixture 実寸の 3024x4032）とバイト列がそのまま届いているはず
    let (width, height) =
      resources.natural_size(&AssetId::new("/project/testimage5.png")).expect("自然寸法が確定するはず");
    assert!((width - 3024.0).abs() < 1e-4, "幅は fixture 実寸と一致するはず: width={width}");
    assert!((height - 4032.0).abs() < 1e-4, "高さは fixture 実寸と一致するはず: height={height}");
    let bytes = resources.into_image_bytes();
    assert_eq!(bytes.len(), 1);
    assert_eq!(
      bytes[&AssetId::new("/project/testimage5.png")].len(),
      expected_len,
      "読み込んだバイト数は登録した fixture のバイト数と一致するはず"
    );
  }

  #[test]
  fn load_image_resources_wraps_missing_path_as_read_image_error() {
    // Arrange — 何も登録していない MemoryProjectSource に存在しないパスを要求する
    let source = MemoryProjectSource::new();
    let paths = vec![AssetId::new("/project/does-not-exist.png")];

    // Act
    let result = load_image_resources(&source, &paths);

    // Assert
    let Err(BuildPdfError::ReadImage { source, .. }) = result else {
      panic!("ReadImage を期待");
    };
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound, "未登録パスは NotFound になるはず");
  }

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
}
