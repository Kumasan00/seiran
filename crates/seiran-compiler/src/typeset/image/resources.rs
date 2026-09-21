//! 画像の読込・自然寸法解決と 1 画像ぶんの表示寸法確定
//!
//! （旧 `pdf_gen::image` → `compiler::image_resources`。epic #276 / #279 / #350 で移設）

use std::collections::HashMap;

use tracing::debug;

use crate::{
  failures::Failures,
  length::Length,
  project::{ProjectPath, ProjectSource},
  publication::ImageFormat,
  typeset::{
    error::TypesetError,
    image::natural_size::{self, NaturalSize},
  },
};

/// 描画へ渡す画像 1 件（判定済みの形式 + 生バイト列）。
#[derive(Debug)]
pub(crate) struct ImageAsset {
  /// 拡張子から判定した画像形式
  pub(crate) format: ImageFormat,
  /// ファイルから読み込んだ生バイト列（未デコード）
  pub(crate) bytes: Vec<u8>,
}

/// 画像パスごとの自然寸法と生バイト列（旧 `pdf_gen::ImageSet`）。
#[derive(Debug)]
pub(crate) struct ImageResources {
  /// パス → 検証済みの自然寸法（ラスタは px、SVG は usvg が報告した width / height）。
  natural_sizes: HashMap<ProjectPath, NaturalSize>,
  /// パス → 判定済みの形式と生バイト列（未デコード）。
  assets: HashMap<ProjectPath, ImageAsset>,
}

impl ImageResources {
  /// `path` の自然寸法を返す。`load_image_resources` に渡さなかったパスは `None`。
  fn natural_size(&self, path: &ProjectPath) -> Option<NaturalSize> { return self.natural_sizes.get(path).copied(); }

  /// 保持していた画像資源を消費して返す。
  ///
  /// `Publication` の描画資源の構築に使う。これを呼んだ後は自然寸法の参照はできない。
  #[must_use]
  pub(crate) fn into_assets(self) -> HashMap<ProjectPath, ImageAsset> { return self.assets; }
}

/// 画像ファイルを読み込み、自然寸法と判定済み形式・生バイト列を格納した [`ImageResources`] を返す。
///
/// 画像ファイルを読む唯一の箇所。`source`（[`crate::project::ProjectSource`]）経由で読み込むため、
/// 本体コードはここでも `std::fs` に直接触れない。ここで保持した資源は
/// [`ImageResources::into_assets`] で取り出し、`Publication` の描画資源へ渡す。
///
/// 画像は互いに独立に読めるので、1 件目で打ち切らず全件を試して失敗を全件返す。`paths` は
/// [`super::collect_image_paths`] が `BTreeSet<ProjectPath>` で作った正規化済みパスの昇順なので、
/// 報告順もそのまま昇順で決定的になる。
///
/// # Errors
///
/// 画像の読み込み・デコードに失敗した場合と、デコーダが報告した自然寸法が有限かつ正でない場合に
/// [`TypesetError`] をパス昇順で返す。
pub(crate) fn load_image_resources(
  source: &dyn ProjectSource,
  paths: &[ProjectPath],
) -> Result<ImageResources, Failures<TypesetError>> {
  let mut natural_sizes = HashMap::with_capacity(paths.len());
  let mut assets = HashMap::with_capacity(paths.len());
  let mut errors = Vec::new();
  for path in paths {
    match read_image(source, path) {
      Ok((natural_size, asset)) => {
        natural_sizes.insert(path.clone(), natural_size);
        assets.insert(path.clone(), asset);
      },
      Err(error) => errors.push(error),
    }
  }
  if let Some(failures) = Failures::from_vec(errors) {
    return Err(failures);
  }
  debug!(image_count = natural_sizes.len(), "画像の自然寸法を確定");
  return Ok(ImageResources {
    natural_sizes,
    assets,
  });
}

/// 画像 1 件を読み込み、自然寸法と描画へ渡す資源を返す。
fn read_image(source: &dyn ProjectSource, path: &ProjectPath) -> Result<(NaturalSize, ImageAsset), TypesetError> {
  let path_string = path.to_string();
  let file_bytes = source.read_bytes(path).map_err(|source| {
    return TypesetError::ReadImage {
      path: path_string.clone(),
      source,
    };
  })?;
  let Some(format) = ImageFormat::from_path(&path_string) else {
    return Err(TypesetError::UnsupportedImageFormat { path: path_string });
  };
  let natural_size = natural_size::natural_image_size(&path_string, format, &file_bytes)?;
  return Ok((
    natural_size,
    ImageAsset {
      format,
      bytes: file_bytes.to_vec(),
    },
  ));
}

/// 画像 1 件の描画寸法を、ソース指定値と自然寸法・段幅から確定する。
///
/// `width` / `height` はソースが指定した値（省略された辺は `None`）。省略された辺を
/// 自然寸法の縦横比から、両方省略なら段幅いっぱいから埋める。自然寸法は
/// [`load_image_resources`] が検証済みなので、この操作は失敗しない。
///
/// # Panics
///
/// `path` が `images` に無い場合に落ちる — 描画対象の画像と読み込む画像は同じ HIR の `Figure` から
/// 作られるので、食い違うのは収集ロジックの不具合だけ（ユーザー入力では起こせない）。
pub(in crate::typeset) fn resolve_image_size(
  images: &ImageResources,
  path: &ProjectPath,
  width: Option<Length>,
  height: Option<Length>,
  column_width: Length,
) -> (Length, Length) {
  let Some(natural) = images.natural_size(path) else {
    unreachable!(
      "描画対象の画像は collect_image_paths が同じ HIR の Figure から全件集め load_image_resources が読み込む: {path}"
    );
  };
  let (width, height) =
    fit_image_size(width.map(Length::to_pt), height.map(Length::to_pt), natural, column_width.to_pt());
  return (Length::pt(width), Length::pt(height));
}

/// 指定値と縦横比から最終描画寸法（pt）を求める。
///
/// `natural` は検証済みなので縦横比は必ず有限で、この計算は失敗しない。
fn fit_image_size(width: Option<f32>, height: Option<f32>, natural: NaturalSize, column_width: f32) -> (f32, f32) {
  let ratio = natural.aspect_ratio();
  return match (width, height) {
    (Some(w), Some(h)) => (w, h),
    (Some(w), None) => (w, w * ratio),
    (None, Some(h)) => (h / ratio, h),
    (None, None) => (column_width, column_width * ratio),
  };
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::*;
  use crate::project::{MemoryProjectSource, SourceReadError};

  /// リポジトリ直下の `tests/image/` にある実 fixture を `CARGO_MANIFEST_DIR` 基準で読む。
  ///
  /// `crates/seiran-compiler` から見て 2 階層上がワークスペースルート（`compiler::test_support::workspace_root` と同じ関係）。
  fn read_image_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image").join(name);
    return std::fs::read(&path).unwrap_or_else(|error| panic!("画像 fixture を読めるはず: {path:?}: {error}"));
  }

  #[test]
  fn load_image_resources_reads_through_project_source() {
    // Arrange — 実 fixture（tests/image/testimage5.png、756x1008 の PNG）のバイト列を
    // MemoryProjectSource に登録する。`with_bytes` に渡す前に長さを控え、後で
    // 「登録したバイト列がそのまま保持されているか」を検証できるようにする
    let png_bytes = read_image_fixture("testimage5.png");
    let expected_len = png_bytes.len();
    let source = MemoryProjectSource::new().with_bytes("/project/testimage5.png", png_bytes);
    let paths = vec![ProjectPath::new("/project/testimage5.png")];

    // Act
    let resources = load_image_resources(&source, &paths).expect("メモリ上の fixture を読めるはず");

    // Assert — 自然寸法（fixture 実寸の 756x1008）とバイト列がそのまま届いているはず
    let natural = resources
      .natural_size(&ProjectPath::new("/project/testimage5.png"))
      .expect("自然寸法が確定するはず");
    let (width, height) = (natural.width(), natural.height());
    assert!((width - 756.0).abs() < 1e-4, "幅は fixture 実寸と一致するはず: width={width}");
    assert!((height - 1008.0).abs() < 1e-4, "高さは fixture 実寸と一致するはず: height={height}");
    let assets = resources.into_assets();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[&ProjectPath::new("/project/testimage5.png")].format, ImageFormat::Png);
    assert_eq!(
      assets[&ProjectPath::new("/project/testimage5.png")].bytes.len(),
      expected_len,
      "読み込んだバイト数は登録した fixture のバイト数と一致するはず"
    );
  }

  #[test]
  fn load_image_resources_wraps_missing_path_as_read_image_error() {
    // Arrange — 何も登録していない MemoryProjectSource に存在しないパスを要求する
    let source = MemoryProjectSource::new();
    let paths = vec![ProjectPath::new("/project/does-not-exist.png")];

    // Act
    let result = load_image_resources(&source, &paths);

    // Assert
    let Err(failures) = result else {
      panic!("読込エラーを期待");
    };
    let TypesetError::ReadImage { source, .. } = failures.first() else {
      panic!("ReadImage を期待");
    };
    assert!(matches!(source, SourceReadError::NotFound), "未登録パスは NotFound になるはず: {source:?}");
  }

  #[test]
  #[should_panic(expected = "描画対象の画像は collect_image_paths が同じ HIR の Figure から全件集め")]
  fn resolve_image_size_panics_when_path_is_absent_from_resources() {
    // Arrange — `collect_image_paths` が `Figure` を取りこぼしたのと同じ状態を作る
    // （本来は同じ HIR 走査で作られるので外部入力では起こせない）
    let resources = load_image_resources(&MemoryProjectSource::new(), &[]).expect("画像 0 件なら成功するはず");

    // Act — 不変条件の破れなので診断ではなく panic する
    let _ =
      resolve_image_size(&resources, &ProjectPath::new("/project/never-loaded.png"), None, None, Length::pt(400.0));
  }

  #[test]
  fn fit_image_size_uses_specified_values_when_both_given() {
    // Arrange
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");

    // Act
    let (w, h) = fit_image_size(Some(80.0), Some(60.0), natural, 400.0);

    // Assert
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_infers_height_from_aspect_when_only_width_given() {
    // Arrange
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");

    // Act
    let (w, h) = fit_image_size(Some(80.0), None, natural, 400.0);

    // Assert
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_infers_width_from_aspect_when_only_height_given() {
    // Arrange
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");

    // Act
    let (w, h) = fit_image_size(None, Some(60.0), natural, 400.0);

    // Assert
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_fits_to_column_when_both_omitted() {
    // Arrange
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");

    // Act
    let (w, h) = fit_image_size(None, None, natural, 400.0);

    // Assert
    assert!((w - 400.0).abs() < 1e-4);
    assert!((h - 300.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_accepts_fractional_svg_natural_size() {
    // Arrange
    let natural = NaturalSize::new(320.5, 180.0).expect("正の有限値");

    // Act
    let (w, h) = fit_image_size(Some(160.0), None, natural, 400.0);

    // Assert
    let expected_height = 160.0 * (180.0f32 / 320.5f32);
    assert!((w - 160.0).abs() < 1e-4);
    assert!((h - expected_height).abs() < 1e-4);
  }
}
