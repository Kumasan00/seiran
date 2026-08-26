//! 描画直前の確定済み中間表現 [`Publication`] — compile の成果物。
//!
//! 座標は pt 単位の `f32`、描画順は配列順で確定している。ここに置くデータ型は**純データ**だけで、
//! 描画バックエンド（`seiran-pdf` / krilla）のハンドルは 1 つも含まない（#372）。
//! フォント・画像は生バイト列と構築設定のまま持ち、krilla フォントの構築は render が行う。
//!
//! データ型は組版の中間型（`crate::typeset::Page` 等）をフィールドに持たない。例外は
//! `crate::typeset::GlyphRun` / [`crate::typeset::FontMetric`] / [`crate::typeset::FontFaceConfig`] /
//! [`crate::typeset::ImageFormat`] で、これらは「シェーピング結果」「フォント計測値」「フォント構築設定」
//! 「判定済みの画像形式」をそのまま描画へ渡す leaf 値なので同型の複製を作らず直接載せる。
//!
//! 唯一の構築経路である子 module `build` はこの制約の外側にある — `crate::typeset::LaidOutDocument` /
//! `Page` / `PlacedBlock` / `PlacedTableRow` / `HBoxContent` / `FontResources` / `ImageAsset` を走査して
//! 上のデータ型へ写すのがその責務だから。組版中間型への依存はこの写像 1 箇所に閉じ、データ型側へは
//! 漏らさない（データ型へ新しいフィールドを足すときは上の例外規則が効く）。
//!
//! # 外部から不正状態を作れないこと（#378）
//!
//! 文書を組み立てる型（[`Publication`] / [`PublicationPage`] / [`PublicationResources`]）と、
//! 不変条件を持つ値（[`Rect`] / [`ImageRef`]）はフィールドを非公開にし、構築経路を `pub(crate)` の
//! コンストラクタへ限定してある。コンストラクタは検証を通った値だけを返すので、`seiran-pdf` は
//! 「描画命令の値が壊れている」ケースを考えなくてよい:
//!
//! - [`Rect`] は幅・高さが非負の有限値（krilla の `Rect::from_xywh` が受け付ける範囲）
//! - [`PublicationPage`] のページ矩形と画像の描画矩形は幅・高さが正（krilla の `Size::from_wh` の要求）
//! - [`PublicationLinkTarget::Internal`] と [`PublicationOutlineEntry`] の到達先ページは必ず存在する
//! - [`PaintOp::DrawImage`] が持つ [`ImageRef`] は必ず [`PublicationResources`] の画像を指す

mod build;

use std::{
  fmt::{self, Debug, Formatter},
  sync::Arc,
};

pub(crate) use build::build;

use crate::{
  project::{FontMap, FontType},
  typeset::{FontFaceConfig, FontMetric, GlyphRun, ImageFormat},
};

/// 座標と描画順が確定した文書。
#[derive(Debug, Clone, PartialEq)]
pub struct Publication {
  /// 確定ページ列（文書順）
  pages: Vec<PublicationPage>,
  /// PDF しおりのフラット列。出力しない場合は `None`
  outline: Option<Vec<PublicationOutlineEntry>>,
  /// PDF メタデータ（`config.document` から前倒し解決済み）
  metadata: PublicationMetadata,
  /// 描画に必要なフォント・画像資源
  resources: PublicationResources,
}

impl Publication {
  /// 確定ページ列・しおり・メタデータ・描画資源から [`Publication`] を構築する。
  ///
  /// 内部リンクとしおりの到達先が実在するページを指していなければ `None` を返す
  /// （描画側が「壊れた到達先」を扱わずに済むようにするための唯一の検査点）。
  pub(crate) fn new(
    pages: Vec<PublicationPage>,
    outline: Option<Vec<PublicationOutlineEntry>>,
    metadata: PublicationMetadata,
    resources: PublicationResources,
  ) -> Option<Self> {
    let page_count = pages.len();
    let links_resolve = pages.iter().flat_map(|page| return &page.links).all(|link| {
      return match &link.target {
        PublicationLinkTarget::Internal(dest) => dest.page_index < page_count,
        PublicationLinkTarget::External(_) => true,
      };
    });
    if !links_resolve {
      return None;
    }
    if let Some(entries) = &outline
      && !entries.iter().all(|entry| return entry.dest.page_index < page_count)
    {
      return None;
    }
    return Some(Publication {
      pages,
      outline,
      metadata,
      resources,
    });
  }

  /// 確定ページ列（文書順）を返す。
  #[must_use]
  pub fn pages(&self) -> &[PublicationPage] { return &self.pages; }

  /// PDF しおりのフラット列を返す。出力しない場合は `None`。
  #[must_use]
  pub fn outline(&self) -> Option<&[PublicationOutlineEntry]> { return self.outline.as_deref(); }

  /// PDF メタデータを返す。
  #[must_use]
  pub fn metadata(&self) -> &PublicationMetadata { return &self.metadata; }

  /// 描画に必要なフォント・画像資源を返す。
  #[must_use]
  pub fn resources(&self) -> &PublicationResources { return &self.resources; }
}

/// 描画に必要なフォント・画像資源（すべて生データ）。
///
/// フォントは 19 種別ぶんが必ず揃う（[`FontMap`] が構築時に保証する）。構築経路は
/// [`PublicationResources::new`] だけで、これは crate 内非公開 — `compile` 以外が
/// `Publication` を組み立てることはできない。
#[derive(Clone, PartialEq)]
pub struct PublicationResources {
  /// フォント種別ごとの描画資源
  fonts: FontMap<PublicationFont>,
  /// 描画対象の画像（パス昇順。[`ImageRef`] はこの配列の添字）
  images: Vec<PublicationImage>,
}

impl PublicationResources {
  /// フォント・画像資源から [`PublicationResources`] を構築する。
  ///
  /// `images` は表示順に依存しない決定的な順序（パス昇順）で渡す。
  pub(crate) fn new(fonts: FontMap<PublicationFont>, images: Vec<PublicationImage>) -> Self {
    return PublicationResources { fonts, images };
  }

  /// 指定フォント種別の描画資源を返す。
  #[must_use]
  pub fn font(&self, font_type: FontType) -> &PublicationFont { return &self.fonts[font_type]; }

  /// 指定パスの画像への参照を返す。保持していないパスは `None`。
  ///
  /// [`ImageRef`] の発行経路はここだけなので、`PaintOp::DrawImage` は必ず実在する画像を指す。
  /// 画像は 1 文書あたり数十件なので、索引を二重に持たず線形探索で引く。
  pub(crate) fn image_ref(&self, path: &str) -> Option<ImageRef> {
    return self.images.iter().position(|image| return image.path == path).map(ImageRef);
  }

  /// 参照先の画像を返す。
  ///
  /// # Panics
  ///
  /// [`ImageRef`] の発行経路は [`PublicationResources::image_ref`] だけで、`images` は構築後に
  /// 変更されないため添字は必ず有効（破れていたら不変条件の破れなのでここで落とす）。
  #[must_use]
  pub fn image(&self, image_ref: ImageRef) -> &PublicationImage {
    let Some(image) = self.images.get(image_ref.0) else {
      unreachable!("ImageRef は PublicationResources::image_ref が発行した添字だけを持つ: {image_ref:?}");
    };
    return image;
  }
}

impl Debug for PublicationResources {
  /// バイト列の中身は出さず、フォント種別ごとの長さと設定・画像のパスと長さだけを出す。
  ///
  /// 生バイト列を出すと（フォント 19 種別 + 画像で数百 MB になり）比較失敗時の出力が読めなくなる。
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    return formatter
      .debug_struct("PublicationResources")
      .field("fonts", &self.fonts)
      .field("images", &self.images)
      .finish();
  }
}

/// 描画対象の画像 1 件（形式判定済みの生バイト列）。
#[derive(Clone, PartialEq, Eq)]
pub struct PublicationImage {
  /// 画像ファイルへのパス（診断メッセージ用）
  pub path: String,
  /// 判定済みの画像形式（判定は `typeset::image` が済ませている）
  pub format: ImageFormat,
  /// ファイルの生バイト列（未デコード）
  pub bytes: Vec<u8>,
}

impl Debug for PublicationImage {
  /// バイト列の中身は出さず、長さだけを出す（[`PublicationResources`] の `Debug` と同じ理由）。
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    return formatter
      .debug_struct("PublicationImage")
      .field("path", &self.path)
      .field("format", &self.format)
      .field("bytes_len", &self.bytes.len())
      .finish();
  }
}

/// [`PublicationResources`] が保持する画像への参照。
///
/// 中身（配列添字）は非公開で、発行経路は [`PublicationResources::image_ref`] だけ。
/// 外部 crate はこの値を作れないので、資源に無い画像を指す描画命令を構築できない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageRef(usize);

/// 1 フォント種別ぶんの描画資源（バイト列 + 構築設定 + 計測値）。
#[derive(Clone, PartialEq)]
pub struct PublicationFont {
  /// フォントファイルのバイト列（同じファイルを指す種別は同一の `Arc` を共有する）
  pub bytes: Arc<Vec<u8>>,
  /// krilla フォント構築に必要な設定（TTC インデックス・バリアブルフォント軸）
  pub face: FontFaceConfig,
  /// 基本メトリクス（フォントユニット系）
  pub metric: FontMetric,
}

impl Debug for PublicationFont {
  /// バイト列の中身は出さず、長さだけを出す（[`PublicationResources`] の `Debug` と同じ理由）。
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    return formatter
      .debug_struct("PublicationFont")
      .field("bytes_len", &self.bytes.len())
      .field("face", &self.face)
      .field("metric", &self.metric)
      .finish();
  }
}

/// 解決済みの PDF メタデータ。
///
/// `title` は `document.title` を優先し、未設定なら `output.name` にフォールバック済み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationMetadata {
  /// 文書タイトル（フォールバック解決済み）
  pub title: String,
  /// 著者名
  pub author: Option<String>,
  /// 主題
  pub subject: Option<String>,
  /// 文書全体の言語（BCP 47）
  pub language: Option<String>,
  /// キーワード
  pub keywords: Option<Vec<String>>,
}

/// 1 ページぶんの確定描画データ
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationPage {
  /// ページ全体の矩形（左上原点）
  page_box: Rect,
  /// 背面から前面への描画順（配列順がそのまま描画順）
  ops: Vec<PaintOp>,
  /// このページのクリック可能なリンク領域（解決済み。到達先の見つからない内部リンクは含まない）
  links: Vec<PublicationLink>,
}

impl PublicationPage {
  /// ページ矩形・描画命令・リンク領域から [`PublicationPage`] を構築する。
  ///
  /// 次のいずれかを満たさない場合は `None` を返す（krilla が `Size::from_wh` で 0 も拒否するため、
  /// [`Rect`] の非負より強い「正」をここで要求する）:
  ///
  /// - ページ矩形の幅・高さが正
  /// - [`PaintOp::DrawImage`] の描画矩形の幅・高さが正
  pub(crate) fn new(page_box: Rect, ops: Vec<PaintOp>, links: Vec<PublicationLink>) -> Option<Self> {
    if !is_positive_size(page_box) {
      return None;
    }
    let image_rects_positive = ops.iter().all(|op| {
      return match op {
        PaintOp::DrawImage { rect, .. } => is_positive_size(*rect),
        PaintOp::DrawGlyphRun { .. } | PaintOp::FillRect { .. } => true,
      };
    });
    if !image_rects_positive {
      return None;
    }
    return Some(PublicationPage {
      page_box,
      ops,
      links,
    });
  }

  /// ページ全体の矩形（左上原点）を返す。
  #[must_use]
  pub fn page_box(&self) -> Rect { return self.page_box; }

  /// 背面から前面への描画命令列を返す。
  #[must_use]
  pub fn ops(&self) -> &[PaintOp] { return &self.ops; }

  /// このページのクリック可能なリンク領域を返す。
  #[must_use]
  pub fn links(&self) -> &[PublicationLink] { return &self.links; }
}

/// 矩形の幅・高さがともに正かを返す（krilla の `Size::from_wh` の受け入れ条件）。
fn is_positive_size(rect: Rect) -> bool { return rect.width > 0.0 && rect.height > 0.0; }

/// 描画命令。
#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
  /// シェーピング済みグリフ列の描画（`origin` はベースライン左端）
  DrawGlyphRun {
    /// 描画原点（ページ左上基準、ベースライン位置）
    origin: Point,
    /// シェーピング結果一式（フォント種別・グリフ・元テキスト・サイズ・色）
    run: GlyphRun,
  },
  /// 画像の描画
  DrawImage {
    /// 描画する画像（[`PublicationResources`] の画像を指す）
    image: ImageRef,
    /// 描画矩形（幅・高さは正）
    rect: Rect,
    /// ラスタ画像のダウンサンプリング上限 DPI（`None` はリサイズなし）
    target_dpi: Option<u32>,
  },
  /// 塗りつぶし矩形（罫線・背景の両方をこれで表す）
  FillRect {
    /// 矩形
    rect: Rect,
    /// 塗り色（RGB）。`None` は既定色（黒）
    color: Option<[u8; 3]>,
  },
}

/// ページ左上原点、右向き・下向きを正とする点（単位: pt）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
  /// 水平座標（pt）
  pub x: f32,
  /// 垂直座標（pt）
  pub y: f32,
}

/// ページ左上原点の矩形（左上角 + 幅 + 高さ、単位: pt）。
///
/// 幅・高さは非負の有限値であることが構築時に保証される（krilla の `Rect::from_xywh` の
/// 受け入れ条件と同じ）。画像・ページのように「0 も許されない」箇所は
/// [`PublicationPage::new`] が追加で検査する。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
  /// 左端の水平座標（pt）
  x: f32,
  /// 上端の垂直座標（pt）
  y: f32,
  /// 幅（pt、非負）
  width: f32,
  /// 高さ（pt、非負）
  height: f32,
}

impl Rect {
  /// 左上角・幅・高さから [`Rect`] を構築する。
  ///
  /// 座標が非有限、または幅・高さが負の場合は `None` を返す。
  pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Option<Self> {
    if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
      return None;
    }
    if width < 0.0 || height < 0.0 {
      return None;
    }
    return Some(Rect {
      x,
      y,
      width,
      height,
    });
  }

  /// 左端の水平座標（pt）を返す。
  #[must_use]
  pub fn x(&self) -> f32 { return self.x; }

  /// 上端の垂直座標（pt）を返す。
  #[must_use]
  pub fn y(&self) -> f32 { return self.y; }

  /// 幅（pt、非負）を返す。
  #[must_use]
  pub fn width(&self) -> f32 { return self.width; }

  /// 高さ（pt、非負）を返す。
  #[must_use]
  pub fn height(&self) -> f32 { return self.height; }
}

/// 解決済みのクリック可能なリンク領域
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationLink {
  /// リンクの行き先
  pub target: PublicationLinkTarget,
  /// クリック可能な矩形
  pub rect: Rect,
}

/// 解決済みのリンク行き先
#[derive(Debug, Clone, PartialEq)]
pub enum PublicationLinkTarget {
  /// 文書内到達先（ページ index + 座標まで解決済み）
  Internal(Destination),
  /// 外部 URI
  External(String),
}

/// 文書内到達先（ページ index + 点）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Destination {
  /// 0 起点のページ index
  pub page_index: usize,
  /// ページ内の到達先座標
  pub point: Point,
}

/// PDF しおりのフラットなエントリ。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationOutlineEntry {
  /// 見出しレベルの深さ（`HeadingLevel::depth()`、0 = Part）
  pub depth: u8,
  /// しおりに表示するテキスト
  pub text: String,
  /// ジャンプ先
  pub dest: Destination,
}

/// テストが描画資源を組み立てるための fixture（本体コードは `build` の構築経路だけを使う）。
///
/// `pub(crate)` なのは `publication` の外の `compiler::dump` のテストも使うため
/// （`Publication` のダンプは描画資源の中身を読まないので、同じダミー資源で足りる）。
#[cfg(test)]
pub(crate) mod test_fixtures {
  use std::sync::Arc;

  use super::{PublicationFont, PublicationImage, PublicationResources};
  use crate::{
    project::{FontMap, FontType},
    typeset::{FontFaceConfig, FontMetric},
  };

  /// 指定した画像だけを持つ描画資源を返す（フォントは全種別ダミーのバイト列 0 個）。
  ///
  /// 座標・寸法に影響しない資源が必要なだけのテスト（ダンプ・`Publication` の組み立て）向け。
  pub(crate) fn resources(images: Vec<PublicationImage>) -> PublicationResources {
    let font = PublicationFont {
      bytes: Arc::new(Vec::new()),
      face: FontFaceConfig {
        font_index: 0,
        variation_axes: None,
      },
      metric: FontMetric {
        upem: 1000.0,
        ascender: 800.0,
        descender: -200.0,
      },
    };
    let fonts = FontMap::from_all(FontType::ALL.iter().map(|_| return font.clone()));
    return PublicationResources::new(fonts, images);
  }
}

#[cfg(test)]
mod tests {
  use super::{
    Destination, ImageRef, PaintOp, Point, Publication, PublicationImage, PublicationLink, PublicationLinkTarget,
    PublicationMetadata, PublicationOutlineEntry, PublicationPage, Rect, test_fixtures::resources,
  };
  use crate::typeset::ImageFormat;

  /// 検証用の最小メタデータを返す。
  fn metadata() -> PublicationMetadata {
    return PublicationMetadata {
      title: "t".to_string(),
      author: None,
      subject: None,
      language: None,
      keywords: None,
    };
  }

  /// A4 相当の正しいページ矩形を返す。
  fn page_box() -> Rect { return Rect::new(0.0, 0.0, 595.0, 842.0).unwrap(); }

  #[test]
  fn rect_new_rejects_negative_size() {
    let negative_width = Rect::new(0.0, 0.0, -1.0, 10.0);
    let negative_height = Rect::new(0.0, 0.0, 10.0, -1.0);
    let zero = Rect::new(0.0, 0.0, 0.0, 0.0);

    assert!(negative_width.is_none(), "負の幅は構築できないはず");
    assert!(negative_height.is_none(), "負の高さは構築できないはず");
    assert!(zero.is_some(), "0 は krilla の Rect::from_xywh が受け付けるので構築できるはず");
  }

  #[test]
  fn rect_new_rejects_non_finite_values() {
    assert!(Rect::new(f32::NAN, 0.0, 1.0, 1.0).is_none(), "NaN 座標は構築できないはず");
    assert!(Rect::new(0.0, 0.0, f32::INFINITY, 1.0).is_none(), "無限大の幅は構築できないはず");
  }

  #[test]
  fn page_new_rejects_zero_sized_page_box() {
    // Arrange
    let degenerate = Rect::new(0.0, 0.0, 0.0, 842.0).unwrap();

    // Act
    let page = PublicationPage::new(degenerate, Vec::new(), Vec::new());

    // Assert
    assert!(page.is_none(), "幅 0 のページ矩形は構築できないはず");
  }

  #[test]
  fn page_new_rejects_zero_sized_image_rect() {
    // Arrange
    let ops = vec![PaintOp::DrawImage {
      image: ImageRef(0),
      rect: Rect::new(0.0, 0.0, 10.0, 0.0).unwrap(),
      target_dpi: None,
    }];

    // Act
    let page = PublicationPage::new(page_box(), ops, Vec::new());

    // Assert
    assert!(page.is_none(), "高さ 0 の画像矩形は構築できないはず");
  }

  #[test]
  fn page_new_accepts_zero_sized_fill_rect() {
    // Arrange — 太さ 0 の罫線（style.toml が非負を許す）は描画されないだけで不正ではない
    let ops = vec![PaintOp::FillRect {
      rect: Rect::new(10.0, 10.0, 100.0, 0.0).unwrap(),
      color: None,
    }];

    // Act
    let page = PublicationPage::new(page_box(), ops, Vec::new());

    // Assert
    assert!(page.is_some(), "0 高さの塗りつぶし矩形は許されるはず");
  }

  #[test]
  fn publication_new_rejects_link_to_missing_page() {
    // Arrange
    let link = PublicationLink {
      target: PublicationLinkTarget::Internal(Destination {
        page_index: 1,
        point: Point { x: 0.0, y: 0.0 },
      }),
      rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
    };
    let pages = vec![PublicationPage::new(page_box(), Vec::new(), vec![link]).unwrap()];

    // Act
    let publication = Publication::new(pages, None, metadata(), resources(Vec::new()));

    // Assert
    assert!(publication.is_none(), "存在しないページを指す内部リンクは構築できないはず");
  }

  #[test]
  fn publication_new_rejects_outline_entry_to_missing_page() {
    // Arrange
    let pages = vec![PublicationPage::new(page_box(), Vec::new(), Vec::new()).unwrap()];
    let outline = Some(vec![PublicationOutlineEntry {
      depth: 0,
      text: "見出し".to_string(),
      dest: Destination {
        page_index: 3,
        point: Point { x: 0.0, y: 0.0 },
      },
    }]);

    // Act
    let publication = Publication::new(pages, outline, metadata(), resources(Vec::new()));

    // Assert
    assert!(publication.is_none(), "存在しないページを指すしおりは構築できないはず");
  }

  #[test]
  fn image_ref_resolves_only_registered_paths() {
    // Arrange
    let resources = resources(vec![PublicationImage {
      path: "a.png".to_string(),
      format: ImageFormat::Png,
      bytes: vec![1, 2, 3],
    }]);

    // Act
    let registered = resources.image_ref("a.png");
    let missing = resources.image_ref("b.png");

    // Assert
    assert!(missing.is_none(), "登録していないパスは ImageRef を得られないはず");
    let image = resources.image(registered.expect("登録済みパスは ImageRef を得られるはず"));
    assert_eq!(image.bytes, vec![1, 2, 3]);
    assert_eq!(image.format, ImageFormat::Png);
  }
}
