//! 画像の読込・自然寸法解決と 1 画像ぶんの表示寸法確定

use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  sync::Arc,
};

use tracing::debug;

use crate::{
  document::ImageFormat,
  failures::Failures,
  length::Length,
  project::{ProjectPath, ProjectSource},
  typeset::{
    error::TypesetError,
    image::{
      manifest::ImageRequest,
      natural_size::{self, NaturalSize},
      pdf,
    },
    warning::TypesetWarning,
  },
};

/// 描画へ渡す画像ファイル 1 件（判定済みの形式 + 生バイト列）。
#[derive(Debug, Clone)]
pub(crate) struct ImageAsset {
  /// 拡張子から判定した画像形式
  pub(in crate::typeset) format: ImageFormat,
  /// ファイルから読み込んだ生バイト列（未デコード。PDF の複数ページで共有する）
  pub(in crate::typeset) bytes: Arc<[u8]>,
}

/// 図にする画像の単位（パス, PDF のページ index）ごとの自然寸法と、ファイルごとの生バイト列。
#[derive(Debug)]
pub(crate) struct ImageResources {
  /// （パス, ページ index）→ 検証済みの自然寸法（ラスタは px、SVG は usvg が報告した width / height、PDF は pt）。
  /// ページ index は [`pdf_page_index`] の値で、PDF 以外は `None`。
  natural_sizes: BTreeMap<(ProjectPath, Option<usize>), NaturalSize>,
  /// パス → 判定済みの形式と生バイト列（未デコード）。
  assets: HashMap<ProjectPath, ImageAsset>,
}

impl ImageResources {
  /// 図の単位の自然寸法を返す。`load_image_resources` に渡さなかった単位は `None`。
  fn natural_size(&self, path: &ProjectPath, page_index: Option<usize>) -> Option<NaturalSize> {
    return self.natural_sizes.get(&(path.clone(), page_index)).copied();
  }

  /// 保持していた画像資源を、図の単位ごとに（パス, ページ index）の昇順で消費して返す。
  #[must_use]
  pub(in crate::typeset) fn into_drawables(self) -> Vec<(ProjectPath, Option<usize>, ImageAsset)> {
    let ImageResources {
      natural_sizes,
      assets,
    } = self;
    return natural_sizes
      .into_keys()
      .map(|(path, page_index)| {
        let Some(asset) = assets.get(&path) else {
          unreachable!("自然寸法を登録した図の単位は、同じ読込で資源も登録している: {path}");
        };
        let asset = asset.clone();
        return (path, page_index, asset);
      })
      .collect();
  }
}

/// 図が使う PDF のページ index（0 始まり）。PDF 以外は `None`。
///
/// `page` を省略できるのは 1 ページの PDF だけなので、省略は先頭ページで、中身を読まずに決まる
/// （2 ページ以上での省略は [`load_image_resources`] が拒否する）。`page` は frontend が正整数だけを通す。
pub(in crate::typeset) fn pdf_page_index(format: ImageFormat, page: Option<u32>) -> Option<usize> {
  return match format {
    ImageFormat::Pdf => Some(page.map_or(0, |number| return number as usize - 1)),
    ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Svg => None,
  };
}

/// 画像ファイルを読み込み、自然寸法と判定済み形式・生バイト列を格納した [`ImageResources`] と、
/// 取り込みで落とした内容の警告（パス → ページの昇順）を返す。
///
/// `requests` は [`super::collect_image_requests`] が作った正規化済みパスの昇順で、1 ファイルの中の
/// `page` も昇順なので、失敗の報告順も決定的になる。1 ファイルの中のページ検査は、失敗しても残りの
/// ページを続ける（互いに独立）。
///
/// # Errors
///
/// 画像の読み込み・デコード・PDF の検査とページの選択に失敗した場合と、デコーダが報告した自然寸法が
/// 有限かつ正でない場合に [`TypesetError`] を返す。
pub(crate) fn load_image_resources(
  source: &dyn ProjectSource,
  requests: &[ImageRequest],
) -> Result<(ImageResources, Vec<TypesetWarning>), Failures<TypesetError>> {
  let mut natural_sizes = BTreeMap::new();
  let mut assets = HashMap::with_capacity(requests.len());
  let mut annotated_pages = BTreeSet::new();
  let mut errors = Vec::new();
  for request in requests {
    let path_string = request.path.to_string();
    let bytes = match source.read_bytes(&request.path) {
      Ok(bytes) => bytes,
      Err(source) => {
        errors.push(TypesetError::ReadImage {
          path: path_string,
          source,
        });
        continue;
      },
    };
    if request.format == ImageFormat::Pdf {
      let file = match pdf::open(&path_string, &bytes) {
        Ok(file) => file,
        Err(error) => {
          errors.push(error);
          continue;
        },
      };
      for &page in &request.pages {
        match file.page(&path_string, page) {
          Ok(selected) => {
            let page_index = pdf_page_index(request.format, page);
            natural_sizes.insert((request.path.clone(), page_index), selected.natural_size);
            if selected.has_annotations {
              annotated_pages.insert((request.path.clone(), page.unwrap_or(1)));
            }
          },
          Err(error) => errors.push(error),
        }
      }
    } else {
      match natural_size::natural_image_size(&path_string, request.format, &bytes) {
        Ok(size) => {
          natural_sizes.insert((request.path.clone(), None), size);
        },
        Err(error) => {
          errors.push(error);
          continue;
        },
      }
    }
    assets.insert(
      request.path.clone(),
      ImageAsset {
        format: request.format,
        bytes,
      },
    );
  }
  if let Some(failures) = Failures::from_vec(errors) {
    return Err(failures);
  }
  debug!(image_count = natural_sizes.len(), "画像の自然寸法を確定");
  let warnings = annotated_pages
    .into_iter()
    .map(|(path, page)| return TypesetWarning::PdfAnnotationsDropped { path, page })
    .collect();
  return Ok((
    ImageResources {
      natural_sizes,
      assets,
    },
    warnings,
  ));
}

/// 図 1 件の描画寸法を、ソース指定値と自然寸法・利用幅から確定する。
///
/// `page_index` は [`pdf_page_index`] の値。`width` / `height` はソースが指定した値（省略された辺は `None`）。
/// 省略された辺を自然寸法の縦横比から、両方省略なら利用幅 `available`（段幅から引用・リストの字下げを引いた幅。
/// 字下げが使い切ったときは呼び元が段幅を渡すので正）いっぱいから埋める。自然寸法は
/// [`load_image_resources`] が検証済みなので、この操作は失敗しない。
///
/// # Panics
///
/// （`path`, `page_index`）が `images` に無い場合に落ちる — 描画対象の図と読み込む図は同じ HIR の `Figure` から
/// 作られるので、食い違うのは収集ロジックの不具合だけ（ユーザー入力では起こせない）。
pub(in crate::typeset) fn resolve_image_size(
  images: &ImageResources,
  path: &ProjectPath,
  page_index: Option<usize>,
  width: Option<Length>,
  height: Option<Length>,
  available: Length,
) -> (Length, Length) {
  let Some(natural) = images.natural_size(path, page_index) else {
    unreachable!(
      "描画対象の画像は collect_image_requests が同じ HIR の Figure から全件集め load_image_resources が読み込む: {path} {page_index:?}"
    );
  };
  let (width, height) = fit_image_size(width.map(Length::to_pt), height.map(Length::to_pt), natural, available.to_pt());
  return (Length::pt(width), Length::pt(height));
}

/// 指定値と縦横比から最終描画寸法（pt）を求める。
fn fit_image_size(width: Option<f32>, height: Option<f32>, natural: NaturalSize, available: f32) -> (f32, f32) {
  let ratio = natural.aspect_ratio();
  return match (width, height) {
    (Some(w), Some(h)) => (w, h),
    (Some(w), None) => (w, w * ratio),
    (None, Some(h)) => (h / ratio, h),
    (None, None) => (available, available * ratio),
  };
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::*;
  use crate::project::{MemoryProjectSource, ProjectSourceError};

  /// リポジトリ直下の `tests/image/` にある実 fixture を `CARGO_MANIFEST_DIR` 基準で読む。
  ///
  /// `crates/seiran-compiler` から見て 2 階層上がワークスペースルート。
  fn read_image_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image").join(name);
    return std::fs::read(&path).unwrap_or_else(|error| panic!("画像 fixture を読めるはず: {path:?}: {error}"));
  }

  /// ページを指定しない（非 PDF の）1 ファイルぶんの読込要求を組み立てる
  fn request(path: &str, format: ImageFormat) -> ImageRequest { return pdf_request(path, format, &[None]); }

  /// ソースに書かれた `page` の集合つきの読込要求を組み立てる
  fn pdf_request(path: &str, format: ImageFormat, pages: &[Option<u32>]) -> ImageRequest {
    return ImageRequest {
      path: ProjectPath::new(path),
      format,
      pages: pages.iter().copied().collect(),
    };
  }

  /// `tests/image/` の PDF fixture を `/project/<name>` に登録した資源と、その要求を読む
  fn load_pdf(
    name: &str,
    pages: &[Option<u32>],
  ) -> Result<(ImageResources, Vec<TypesetWarning>), Failures<TypesetError>> {
    let path = format!("/project/{name}");
    let source = MemoryProjectSource::new().with_bytes(&path, read_image_fixture(name));
    return load_image_resources(&source, &[pdf_request(&path, ImageFormat::Pdf, pages)]);
  }

  #[test]
  fn load_image_resources_dedupes_omitted_and_first_page() {
    // 1 ページの PDF を page 省略と page=1 で参照しても同じページなので、自然寸法は 1 件にまとまる
    let (resources, warnings) = load_pdf("vector1.pdf", &[None, Some(1)]).expect("1 ページの PDF は読めるはず");

    let drawables = resources.into_drawables();
    let keys: Vec<_> = drawables.iter().map(|(path, page_index, _)| return (path.to_string(), *page_index)).collect();
    assert_eq!(keys, vec![("/project/vector1.pdf".to_string(), Some(0))]);
    assert!(warnings.is_empty());
  }

  #[test]
  fn inspect_reports_every_bad_request_of_one_file() {
    let result = load_pdf("vector2.pdf", &[None, Some(2), Some(9)]);

    let Err(failures) = result else {
      panic!("ページ指定の誤りを期待");
    };
    let errors: Vec<_> = failures.into_iter().collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(matches!(errors[0], TypesetError::PdfPageRequired { page_count: 2, .. }), "{errors:?}");
    assert!(
      matches!(
        errors[1],
        TypesetError::PdfPageOutOfRange {
          page: 9,
          page_count: 2,
          ..
        }
      ),
      "{errors:?}"
    );
  }

  #[test]
  fn load_image_resources_warns_about_annotations_once_per_page() {
    let (_, warnings) = load_pdf("annotated.pdf", &[None, Some(1)]).expect("注釈付きの PDF も読めるはず");

    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(matches!(&warnings[0], TypesetWarning::PdfAnnotationsDropped { page: 1, .. }), "{warnings:?}");
  }

  #[test]
  fn load_image_resources_keys_pdf_pages_by_index() {
    let (resources, _) = load_pdf("vector2.pdf", &[Some(1), Some(2)]).expect("2 ページとも選べるはず");

    let path = ProjectPath::new("/project/vector2.pdf");
    assert_eq!(resources.natural_size(&path, Some(0)), NaturalSize::new(300.0, 150.0));
    assert_eq!(resources.natural_size(&path, Some(1)), NaturalSize::new(90.0, 110.0));
  }

  #[test]
  fn load_image_resources_reads_through_project_source() {
    let png_bytes = read_image_fixture("testimage5.png");
    let expected_len = png_bytes.len();
    let source = MemoryProjectSource::new().with_bytes("/project/testimage5.png", png_bytes);
    let paths = vec![request("/project/testimage5.png", ImageFormat::Png)];

    let (resources, _) = load_image_resources(&source, &paths).expect("メモリ上の fixture を読めるはず");

    let natural = resources.natural_size(&ProjectPath::new("/project/testimage5.png"), None);
    assert_eq!(natural, NaturalSize::new(756.0, 1008.0), "自然寸法は fixture 実寸と一致するはず");
    let drawables = resources.into_drawables();
    assert_eq!(drawables.len(), 1);
    let (path, page_index, asset) = &drawables[0];
    assert_eq!(path, &ProjectPath::new("/project/testimage5.png"));
    assert_eq!(*page_index, None);
    assert_eq!(asset.format, ImageFormat::Png);
    assert_eq!(asset.bytes.len(), expected_len, "読み込んだバイト数は登録した fixture のバイト数と一致するはず");
  }

  #[test]
  fn load_image_resources_wraps_missing_path_as_read_image_error() {
    let source = MemoryProjectSource::new();
    let paths = vec![request("/project/does-not-exist.png", ImageFormat::Png)];

    let result = load_image_resources(&source, &paths);

    let Err(failures) = result else {
      panic!("読込エラーを期待");
    };
    let TypesetError::ReadImage { source, .. } = failures.first() else {
      panic!("ReadImage を期待");
    };
    assert!(matches!(source, ProjectSourceError::NotFound), "未登録パスは NotFound になるはず: {source:?}");
  }

  #[test]
  #[should_panic(expected = "描画対象の画像は collect_image_requests が同じ HIR の Figure から全件集め")]
  fn resolve_image_size_panics_when_path_is_absent_from_resources() {
    // `collect_image_requests` が `Figure` を取りこぼしたのと同じ状態を作る
    let (resources, _) = load_image_resources(&MemoryProjectSource::new(), &[]).expect("画像 0 件なら成功するはず");

    let _ = resolve_image_size(
      &resources,
      &ProjectPath::new("/project/never-loaded.png"),
      None,
      None,
      None,
      Length::pt(400.0),
    );
  }

  #[test]
  fn fit_image_size_uses_specified_values_when_both_given() {
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");
    let (w, h) = fit_image_size(Some(80.0), Some(60.0), natural, 400.0);
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_infers_height_from_aspect_when_only_width_given() {
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");
    let (w, h) = fit_image_size(Some(80.0), None, natural, 400.0);
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_infers_width_from_aspect_when_only_height_given() {
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");
    let (w, h) = fit_image_size(None, Some(60.0), natural, 400.0);
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn fit_image_size_fits_to_column_when_both_omitted() {
    let natural = NaturalSize::new(800.0, 600.0).expect("正の有限値");
    let (w, h) = fit_image_size(None, None, natural, 400.0);
    assert!((w - 400.0).abs() < 1e-4);
    assert!((h - 300.0).abs() < 1e-4);
  }
}
