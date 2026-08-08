//! 組版パスのエラー型 [`TypesetError`]
//!
//! 画像資源の読込・デコード・寸法確定で起きる失敗を持つ（#350 で `compiler::error` から移設。
//! `code` は移設前の値をそのまま保つ — 診断の出方を変えないため）。

use miette::Diagnostic;
use seiran_pdf::PdfGenError;
use thiserror::Error;

use crate::project::ProjectPath;

/// 組版の不変条件違反（ユーザー入力に起因しない内部バグ）。
///
/// [`TypesetError`] の他バリアントが表す「設定・入力の誤り」とは型を分け、
/// 呼び出し元が両者を混同して同じ助言文で案内しないようにする。
#[derive(Debug, Error, Diagnostic)]
#[error("内部エラー: {message}")]
#[diagnostic(code(build::internal_bug), help("再現手順とともに issue を報告してください。"))]
pub(crate) struct TypesetBug {
  /// エラーメッセージ
  message: String,
}

impl TypesetBug {
  /// 新しい `TypesetBug` を構築する
  pub(super) fn new(message: impl Into<String>) -> Self {
    return TypesetBug {
      message: message.into(),
    };
  }
}

/// 組版パス（画像資源の解決を含む）で起きるエラー型。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum TypesetError {
  /// シェーパー構築の失敗（`font::FontResources::system` に由来）。
  ///
  /// `FontSystemError` は移設前も `?` でそのまま `miette::Report` になっていたので、
  /// `transparent` でメッセージ・code・help・label・related をすべて内側へ委譲し、
  /// 診断の出方を変えない（#352）。
  #[error(transparent)]
  #[diagnostic(transparent)]
  Font(#[from] super::font::FontSystemError),

  /// 脚注のページ単位採番が上限回数で収束しないエラー
  ///
  /// 不整合なページ列は採用せず、回避策付きの診断を返す。
  #[error("脚注のページ単位採番が {passes} 回の組版で収束しませんでした。")]
  #[diagnostic(
    code(build::footnote::per_page_not_converged),
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
  #[diagnostic(code(build::image::read_image), help("画像ファイルのパスと読み取り権限を確認してください。"))]
  ReadImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の読込エラー。
    #[source]
    source: std::io::Error,
  },

  /// 画像ファイルのデコードに失敗しました。
  #[error("画像ファイルのデコードに失敗しました: {path}")]
  #[diagnostic(
    code(build::image::load_image),
    help("画像ファイルが破損していないか、対応形式（PNG / JPEG / SVG）か確認してください。")
  )]
  LoadImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の `seiran_pdf` デコードエラー。
    #[source]
    #[diagnostic_source]
    source: PdfGenError,
  },

  /// 画像の自然寸法が不正です（縦横比を算出できません）。
  #[error("画像の自然寸法が不正です: {path} (width={width}, height={height})")]
  #[diagnostic(
    code(build::image::invalid_natural_size),
    help("画像ファイルが破損していないか、または width / height を明示指定してください。")
  )]
  InvalidImageNaturalSize {
    /// 画像ファイルのパス。
    path: ProjectPath,
    /// 自然幅（ラスタはピクセル、SVG は pt）。
    width: f32,
    /// 自然高さ（ラスタはピクセル、SVG は pt）。
    height: f32,
  },

  /// 組版の不変条件違反（ユーザー向け診断とは別の型・経路）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Bug(#[from] TypesetBug),
}
