//! PDF 図版の読込とページの選択 — hayro-syntax の使用はこの module に閉じる。
//!
//! 開く時点で暗号化（空パスワードで開けるものだけ受理）・版（PDF 2.0 は拒否）・ページ数（0 は拒否）を
//! 検査し、選んだページの自然寸法と注釈の有無を返す。版をここで拒否するので、描画側（krilla）の
//! 版の不一致エラーには到達しない。
//!
//! 自然寸法は `CropBox` と `MediaBox` の共通部分に `/Rotate` を適用した寸法（pt）で、krilla の
//! `draw_pdf_page` が拡大率の基準にする hayro の `render_dimensions()` と同じ領域。ただし拒否するのは辺が
//! 正の有限値でないページだけなので、hayro が基準を差し替える範囲では組版と描画の基準が食い違う — 辺が
//! 1/4096pt 未満なら hayro は面積 0 とみなして A4 寸法を、1pt 未満なら 1pt を基準にし、図の中身は描画矩形に
//! 対してその比だけずれた倍率で描かれる。

use std::sync::Arc;

use hayro_syntax::{
  LoadPdfError, Pdf, PdfVersion,
  object::{Array, Object, dict::keys::ANNOTS},
  page::{Page, Rotation},
};

use crate::typeset::{error::TypesetError, image::natural_size::NaturalSize};

/// 開いた PDF（版・暗号化・ページ数の検査を通ったもの）。
pub(super) struct PdfFile {
  /// hayro が解析した文書（ページが 1 枚以上ある）
  pdf: Pdf,
}

/// 選んだページについて組版が使う事実。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PdfPage {
  /// 表示範囲（`CropBox` ∩ `MediaBox`）を回転した寸法（pt）
  pub(super) natural_size: NaturalSize,
  /// ページが注釈（`/Annots` の空でない配列）を持つか
  pub(super) has_annotations: bool,
}

/// PDF のバイト列を開き、取り込める文書か検査する。`path` はエラーメッセージにのみ使う。
///
/// # Errors
///
/// 読めない・パスワードが要る / 未対応の暗号化・PDF 2.0・ページが無い場合に [`TypesetError`] を返す。
pub(super) fn open(path: &str, bytes: &Arc<[u8]>) -> Result<PdfFile, TypesetError> {
  // `Arc<[u8]>` を 1 段包むと hayro の `PdfData` が受け取る Sized な `AsRef<[u8]>` になり、バイト列を複製しない
  let pdf = Pdf::new(Arc::new(Arc::clone(bytes))).map_err(|error| {
    return match error {
      LoadPdfError::Decryption(decryption) => TypesetError::EncryptedPdf {
        path: path.to_string(),
        reason: format!("{decryption:?}"),
      },
      // hayro はこれ以上の理由を報告しない
      LoadPdfError::Invalid => TypesetError::ParsePdf {
        path: path.to_string(),
      },
    };
  })?;
  if pdf.version() == PdfVersion::Pdf20 {
    return Err(TypesetError::UnsupportedPdfVersion {
      path: path.to_string(),
    });
  }
  if pdf.pages().is_empty() {
    return Err(TypesetError::EmptyPdf {
      path: path.to_string(),
    });
  }
  return Ok(PdfFile { pdf });
}

impl PdfFile {
  /// ソースに書かれた `page`（1 始まり。省略は `None`）のページを選び、その事実を返す。
  ///
  /// # Errors
  ///
  /// 2 ページ以上で省略された・ページ数を超えた・表示範囲の面積が 0 の場合に [`TypesetError`] を返す。
  pub(super) fn page(&self, path: &str, page: Option<u32>) -> Result<PdfPage, TypesetError> {
    let pages = self.pdf.pages();
    let page_count = pages.len();
    if page.is_none() && page_count >= 2 {
      return Err(TypesetError::PdfPageRequired {
        path: path.to_string(),
        page_count,
      });
    }
    // `page` は frontend が正整数だけを通す（`opt_args::positive_int`）ので 1 を引いても負にならない
    let number = page.unwrap_or(1);
    let Some(selected) = pages.get(number as usize - 1) else {
      return Err(TypesetError::PdfPageOutOfRange {
        path: path.to_string(),
        page: number,
        page_count,
      });
    };
    let natural_size = displayed_size(selected).ok_or_else(|| {
      return TypesetError::EmptyPdfPage {
        path: path.to_string(),
        page: number,
      };
    })?;
    return Ok(PdfPage {
      natural_size,
      has_annotations: has_annotations(selected),
    });
  }
}

/// 表示範囲（`CropBox` ∩ `MediaBox`）に `/Rotate` を適用した寸法。面積 0 なら `None`。
#[expect(
  clippy::cast_possible_truncation,
  reason = "PDF の座標は実用上 f32 の範囲に収まり、描画側の krilla も f32 で扱う"
)]
fn displayed_size(page: &Page<'_>) -> Option<NaturalSize> {
  let visible = page.intersected_crop_box();
  let (width, height) = (visible.width() as f32, visible.height() as f32);
  return match page.rotation() {
    Rotation::None | Rotation::Flipped => NaturalSize::new(width, height),
    Rotation::Horizontal | Rotation::FlippedHorizontal => NaturalSize::new(height, width),
  };
}

/// ページ辞書の `/Annots` が空でない配列か。
fn has_annotations(page: &Page<'_>) -> bool {
  return page
    .raw()
    .get::<Array<'_>>(ANNOTS)
    .is_some_and(|annots| return annots.iter::<Object<'_>>().next().is_some());
}

#[cfg(test)]
mod tests {
  use std::{path::Path, sync::Arc};

  use super::{PdfFile, open};
  use crate::typeset::{error::TypesetError, image::natural_size::NaturalSize};

  /// リポジトリ直下の `tests/image/` にある PDF fixture のバイト列を読む。
  fn fixture_bytes(name: &str) -> Arc<[u8]> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image").join(name);
    return std::fs::read(&path)
      .unwrap_or_else(|error| panic!("fixture を読めるはず: {path:?}: {error}"))
      .into();
  }

  /// PDF fixture を開く。
  fn fixture(name: &str) -> Result<PdfFile, TypesetError> { return open(name, &fixture_bytes(name)); }

  /// 開けるはずの fixture を開く。
  fn opened(name: &str) -> PdfFile {
    return fixture(name).unwrap_or_else(|error| panic!("{name} は開けるはず: {error:?}"));
  }

  /// 選べるはずのページの自然寸法。
  fn size(name: &str, page: Option<u32>) -> NaturalSize {
    return opened(name)
      .page(name, page)
      .unwrap_or_else(|error| panic!("{name} {page:?}: {error:?}"))
      .natural_size;
  }

  #[test]
  fn open_accepts_single_page_vector_pdf() {
    let page = opened("vector1.pdf").page("vector1.pdf", None).expect("1 ページなら page を省略できるはず");

    assert_eq!(page.natural_size, NaturalSize::new(200.0, 120.0).unwrap());
    assert!(!page.has_annotations);
  }

  #[test]
  fn open_accepts_pdf_encrypted_with_empty_user_password() {
    assert_eq!(size("encrypted_owner_only.pdf", None), NaturalSize::new(200.0, 120.0).unwrap());
  }

  #[test]
  fn open_accepts_pdf_1_4() {
    assert_eq!(size("vector1_pdf14.pdf", None), NaturalSize::new(200.0, 120.0).unwrap());
  }

  #[test]
  fn natural_size_intersects_crop_and_media_then_rotates() {
    // p2: MediaBox [0 0 200 100] ∩ CropBox [-50 10 110 160] = 110×90、/Rotate 90 で 90×110
    assert_eq!(size("vector2.pdf", Some(2)), NaturalSize::new(90.0, 110.0).unwrap());
    assert_eq!(size("vector2.pdf", Some(1)), NaturalSize::new(300.0, 150.0).unwrap());
  }

  #[test]
  fn page_requires_explicit_number_on_multi_page_pdf() {
    let result = opened("vector2.pdf").page("vector2.pdf", None);

    assert!(matches!(result, Err(TypesetError::PdfPageRequired { page_count: 2, .. })), "{result:?}");
  }

  #[test]
  fn page_beyond_count_reports_page_count() {
    let result = opened("vector2.pdf").page("vector2.pdf", Some(3));

    assert!(
      matches!(
        result,
        Err(TypesetError::PdfPageOutOfRange {
          page: 3,
          page_count: 2,
          ..
        })
      ),
      "{result:?}"
    );
  }

  #[test]
  fn page_one_is_accepted_on_single_page_pdf() {
    assert_eq!(size("vector1.pdf", Some(1)), NaturalSize::new(200.0, 120.0).unwrap());
  }

  #[test]
  fn zero_area_page_is_rejected() {
    let result = opened("zero_area.pdf").page("zero_area.pdf", None);

    assert!(matches!(result, Err(TypesetError::EmptyPdfPage { page: 1, .. })), "{result:?}");
  }

  #[test]
  fn annotations_are_detected() {
    let page = opened("annotated.pdf").page("annotated.pdf", None).expect("注釈付きのページも選べるはず");

    assert!(page.has_annotations);
  }

  #[test]
  fn open_rejects_password_protected_pdf() {
    let result = fixture("encrypted_user.pdf");

    assert!(matches!(result, Err(TypesetError::EncryptedPdf { .. })), "{:?}", result.err());
  }

  #[test]
  fn open_rejects_pdf_2_0_by_header_and_by_catalog() {
    for name in ["pdf20_header.pdf", "pdf20_catalog.pdf"] {
      let result = fixture(name);

      assert!(matches!(result, Err(TypesetError::UnsupportedPdfVersion { .. })), "{name}: {:?}", result.err());
    }
  }

  #[test]
  fn open_rejects_pdf_without_pages() {
    let result = fixture("no_pages.pdf");

    assert!(matches!(result, Err(TypesetError::EmptyPdf { .. })), "{:?}", result.err());
  }

  #[test]
  fn open_rejects_non_pdf_bytes() {
    let result = fixture("png_bytes.pdf");

    assert!(matches!(result, Err(TypesetError::ParsePdf { .. })), "{:?}", result.err());
  }
}
