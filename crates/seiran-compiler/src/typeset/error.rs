//! 組版パスのエラー型 [`TypesetError`] と [`ComposeError`]
//!
//! フォント資源の構築・画像資源の読込 / デコード / PDF のページ選択 / 自然寸法の検証・ページ単位脚注採番の非収束で起きる失敗
//! [`TypesetError`] と、`compose` の失敗を運ぶ [`ComposeError`] を持つ。長さの演算の溢れは、組版の内側では要素の
//! 起点だけを運ぶ [`ElementOverflow`] で伝え、`pagination::paginate` が位置を解決した [`DimensionOverflow`] にする。

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  document::{NodeId, SourceLocation, SourceMap},
  failures::Failures,
  length::MAX_ABS_PT,
  project::ProjectSourceError,
  source::SourceId,
  typeset::{font::FontSystemError, glyph_issues::TextGlyphError},
};

/// [`compose`](fn@crate::typeset::compose) の失敗
///
/// ソース位置に帰属しない組版の失敗と、ソース位置に帰属しうる字の欠落（[`TextGlyphError::source_id`]）・寸法の溢れ
/// （[`DimensionOverflow::source_id`]）を分けて運ぶ制御フロー型で、表示単位ではない（`miette::Diagnostic` を実装しない）。
/// 後者のうち位置を持つものだけに `compiler` がソース本文を添える。
#[derive(Debug)]
pub(crate) enum ComposeError {
  /// フォント資源の構築・画像資源・ページ単位脚注採番の失敗
  Typeset(Failures<TypesetError>),
  /// 文書が使った字のフォントでの欠落（数式フォントの欠落を文書順に、続けて本文の枠の欠落）
  Glyphs(Failures<TextGlyphError>),
  /// 組版で求めた寸法が長さの値域に収まらなかった（溢れた時点で組版を止めるので 1 件）
  Dimension(DimensionOverflow),
}

/// 長さの演算が溢れた要素 — 組版の内側で溢れを運ぶ型
///
/// 位置の解決は `pagination::paginate` がはみ出し警告と同じ `SourceMap` で行う（[`ElementOverflow::locate`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) struct ElementOverflow {
  /// 溢れた要素。生成物・ページ単位の計算・emit での溢れは `None`
  pub(in crate::typeset) origin: Option<NodeId>,
}

impl ElementOverflow {
  /// 起点の要素を `source_map` で引いて、位置付きの診断にする
  pub(in crate::typeset) fn locate(self, source_map: &SourceMap) -> DimensionOverflow {
    return DimensionOverflow::new(self.origin.map(|origin| return source_map.location(origin)));
  }
}

/// 組版で求めた寸法が、長さの値域に収まりませんでした。
#[derive(Debug, Error, Diagnostic)]
#[error("組版で求めた寸法が扱える範囲（絶対値が {max}pt 未満）を超えました", max = MAX_ABS_PT)]
#[diagnostic(
  code(typeset::length::out_of_range),
  help(
    "この要素の長さの引数（\\space など）か、style.toml のフォントサイズ・行送りの倍率（[text] line_height_factor など）・アキ、config.toml の用紙寸法を小さくしてください"
  )
)]
pub(crate) struct DimensionOverflow {
  /// 溢れた要素が帰属するソース。要素に帰属しない溢れは `None`
  source_id: Option<SourceId>,
  /// 要素の開始位置（`source_id` と同時に `None`）
  #[label("この要素を組む途中で寸法が溢れました")]
  at: Option<SourceSpan>,
}

impl DimensionOverflow {
  /// 要素の位置 `location`（要素に帰属しない溢れは `None`）から作る。ラベルは開始位置に長さ 0 で置く
  pub(crate) fn new(location: Option<SourceLocation>) -> Self {
    return DimensionOverflow {
      source_id: location.map(|location| return location.source_id),
      at: location.map(|location| return location.span.at_start().into()),
    };
  }

  /// 溢れた要素が帰属するソース。要素に帰属しない溢れは `None`
  pub(crate) fn source_id(&self) -> Option<SourceId> { return self.source_id; }
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

  /// SVG の `font-family` が設定済みフォントのどれにも一致しませんでした。
  #[error("SVG の文字に使うフォントが見つかりません: {path}（font-family: {families}）")]
  #[diagnostic(
    code(typeset::image::svg_font_not_found),
    help(
      "font-family には config.toml で設定したフォントのファミリ名（{available}）か serif / sans-serif / monospace を指定してください。"
    )
  )]
  SvgFontNotFound {
    /// 画像ファイルのパス。
    path: String,
    /// 一致しなかった `font-family` のリスト（CSS の綴り）。
    families: String,
    /// 照合できるファミリ名（設定の宣言順、`、` 区切り）。
    available: String,
  },

  /// SVG の文字を、選んだフォントの系統のどの候補でも組めませんでした。
  #[error("SVG の文字を組めるフォントがありません: {path}（{chars}）")]
  #[diagnostic(
    code(typeset::image::svg_missing_glyph),
    help(
      "font-family で選んだ系統（serif / sans-serif / monospace）の枠の候補に、この文字を持つフォントを加えてください。"
    )
  )]
  SvgMissingGlyph {
    /// 画像ファイルのパス。
    path: String,
    /// 組めなかった字（出現順・重複除去。`「字」(U+XXXX)` を `、` で連結）。
    chars: String,
  },

  /// PDF として読み込めませんでした（破損・別形式のファイル）。
  #[error("PDF として読み込めませんでした: {path}")]
  #[diagnostic(
    code(typeset::image::parse_pdf),
    help("ファイルが破損していないか、拡張子が .pdf の別形式のファイルでないか確認してください。")
  )]
  ParsePdf {
    /// 画像ファイルのパス。
    path: String,
  },

  /// パスワードが必要、または未対応の方式で暗号化された PDF です。
  #[error("暗号化された PDF を開けませんでした: {path}（{reason}）")]
  #[diagnostic(code(typeset::image::encrypted_pdf), help("暗号化を外して保存し直してください。"))]
  EncryptedPdf {
    /// 画像ファイルのパス。
    path: String,
    /// 開けない理由（日本語。パスワード必須か未対応の方式か）。
    reason: &'static str,
  },

  /// 暗号化辞書が壊れていて復号の準備ができない PDF です。
  #[error("PDF の暗号化情報が壊れています: {path}")]
  #[diagnostic(
    code(typeset::image::corrupt_pdf_encryption),
    help("ファイルを修復するか、作成元のアプリケーションから PDF を書き出し直してください。")
  )]
  CorruptPdfEncryption {
    /// 画像ファイルのパス。
    path: String,
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

  /// 選んだページの表示範囲（`CropBox` と `MediaBox` の共通部分）の辺が 1/4096pt 以下（hayro が面積 0 とみなす）です。
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
