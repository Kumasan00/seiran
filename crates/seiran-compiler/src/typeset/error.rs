//! 組版パスのエラー型 [`TypesetError`] と [`ComposeError`]
//!
//! フォント資源の構築・画像資源の読込 / デコード / PDF のページ選択 / 自然寸法の検証・ページ単位脚注採番の非収束で起きる失敗
//! [`TypesetError`] と、`compose` の失敗を運ぶ [`ComposeError`] を持つ。

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  failures::Failures,
  project::ProjectSourceError,
  typeset::{font::FontSystemError, glyph_issues::TextGlyphError},
};

/// [`compose`](fn@crate::typeset::compose) の失敗
///
/// ソース位置に帰属しない組版の失敗と、ソース位置に帰属しうる字の欠落（[`TextGlyphError::source_id`]）を分けて運ぶ
/// 制御フロー型で、表示単位ではない（`miette::Diagnostic` を実装しない）。後者のうち位置を持つものだけに `compiler` が
/// ソース本文を添える。
#[derive(Debug)]
pub(crate) enum ComposeError {
  /// フォント資源の構築・画像資源・ページ単位脚注採番の失敗
  Typeset(Failures<TypesetError>),
  /// 文書が使った字のフォントでの欠落（数式フォントの欠落を文書順に、続けて本文の枠の欠落）
  Glyphs(Failures<TextGlyphError>),
}

/// 組版パス（画像資源の解決を含む）で起きるエラー型。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum TypesetError {
  /// フォント資源の構築（`font::FontSystem::load`）の失敗。
  ///
  /// `transparent` でメッセージ・code・help・label・related をすべて内側へ委譲し、
  /// 診断の出方を変えない。
  #[error(transparent)]
  #[diagnostic(transparent)]
  Font(#[from] FontSystemError),

  /// 脚注のページ単位採番が上限回数で収束しないエラー
  ///
  /// 収束しなかったページ列は採用しない。
  #[error("脚注のページ単位採番が {passes} 回の組版で収束しませんでした。")]
  #[diagnostic(
    code(typeset::footnote::per_page_not_converged),
    help(
      "style.toml の [footnote] を numbering = \"continuous\"（文書通しの採番）に切り替えるか、ページ境界に脚注が集中している箇所の本文量・脚注の長さを調整してください。"
    )
  )]
  PerPageFootnoteNotConverged {
    /// 打ち切った組版パスの回数
    passes: u32,
  },

  /// 画像ファイルを読み込めませんでした。
  #[error("画像ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(typeset::image::read_image), help("画像ファイルのパスと読み取り権限を確認してください。"))]
  ReadImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の読込エラー（低水準 cause）。
    #[source]
    source: ProjectSourceError,
  },

  /// ラスタ画像のデコードに失敗しました。
  #[error("画像ファイルのデコードに失敗しました: {path}")]
  #[diagnostic(code(typeset::image::decode_image), help("画像ファイルが破損していないか確認してください。"))]
  DecodeImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の image クレートのエラー。
    #[source]
    source: image::ImageError,
  },

  /// SVG のパースに失敗しました。
  #[error("SVG のパースに失敗しました: {path}")]
  #[diagnostic(
    code(typeset::image::parse_svg),
    help("SVG ファイルが妥当な XML / SVG であることを確認してください。")
  )]
  ParseSvg {
    /// 画像ファイルのパス。
    path: String,
    /// 元の usvg エラー。
    #[source]
    source: usvg::Error,
  },

  /// PDF として読み込めませんでした（破損・別形式のファイル）。
  #[error("PDF として読み込めませんでした: {path} ({reason})")]
  #[diagnostic(
    code(typeset::image::parse_pdf),
    help("ファイルが破損していないか、拡張子が .pdf の別形式のファイルでないか確認してください。")
  )]
  ParsePdf {
    /// 画像ファイルのパス。
    path: String,
    /// hayro が報告した失敗（`Debug` 表現）。
    reason: String,
  },

  /// パスワードが必要、または未対応の方式で暗号化された PDF です。
  #[error("暗号化された PDF を開けませんでした: {path} ({reason})")]
  #[diagnostic(
    code(typeset::image::encrypted_pdf),
    help("パスワードなしで開けない、または未対応の方式で暗号化された PDF です。暗号化を外して保存し直してください。")
  )]
  EncryptedPdf {
    /// 画像ファイルのパス。
    path: String,
    /// hayro が報告した復号の失敗（`Debug` 表現）。
    reason: String,
  },

  /// 出力（PDF 1.7）より新しい版の PDF です。
  #[error("PDF 2.0 の図版は取り込めません: {path}")]
  #[diagnostic(
    code(typeset::image::unsupported_pdf_version),
    help("出力は PDF 1.7 です。図版を PDF 1.7 以下で書き出し直してください。")
  )]
  UnsupportedPdfVersion {
    /// 画像ファイルのパス。
    path: String,
  },

  /// ページが 1 枚も無い PDF です。
  #[error("ページが 1 枚も無い PDF です: {path}")]
  #[diagnostic(code(typeset::image::empty_pdf), help("図にするページを含む PDF を指定してください。"))]
  EmptyPdf {
    /// 画像ファイルのパス。
    path: String,
  },

  /// 2 ページ以上の PDF で `page` が省略されました。
  #[error("{path} は {page_count} ページあるため、使うページを指定する必要があります")]
  #[diagnostic(
    code(typeset::image::pdf_page_required),
    help("\\image[page=N] で 1〜{page_count} のページ番号を指定してください。")
  )]
  PdfPageRequired {
    /// 画像ファイルのパス。
    path: String,
    /// PDF のページ数（2 以上）。
    page_count: usize,
  },

  /// `page` が PDF のページ数を超えています。
  #[error("{path} に {page} ページ目はありません（{page_count} ページの PDF です）")]
  #[diagnostic(code(typeset::image::pdf_page_out_of_range), help("page には 1〜{page_count} を指定してください。"))]
  PdfPageOutOfRange {
    /// 画像ファイルのパス。
    path: String,
    /// ソースに書かれたページ番号（1 始まり）。
    page: u32,
    /// PDF のページ数。
    page_count: usize,
  },

  /// 選んだページの表示範囲（`CropBox` と `MediaBox` の共通部分）の面積が 0 です。
  #[error("{path} の {page} ページ目は表示範囲の面積が 0 です")]
  #[diagnostic(
    code(typeset::image::empty_pdf_page),
    help("表示範囲（CropBox と MediaBox の共通部分）が面積を持つページを指定してください。")
  )]
  EmptyPdfPage {
    /// 画像ファイルのパス。
    path: String,
    /// ページ番号（1 始まり。`page` を省略した 1 ページの PDF では 1）。
    page: u32,
  },

  /// 画像の自然寸法が不正です（縦横比を算出できません）。
  #[error("画像の自然寸法が不正です: {path} (width={width}, height={height})")]
  #[diagnostic(code(typeset::image::invalid_natural_size), help("画像ファイルが破損していないか確認してください。"))]
  InvalidImageNaturalSize {
    /// 画像ファイルのパス。
    path: String,
    /// 自然幅（ラスタはピクセル、SVG は pt）。
    width: f32,
    /// 自然高さ（ラスタはピクセル、SVG は pt）。
    height: f32,
  },
}
