//! PDF ビルドエラー型の定義

use citation::CitationError;
use config::LayoutValidationError;
use frontend::ParseSourceError;
use miette::{Diagnostic, NamedSource};
use model::AssetId;
use thiserror::Error;
use typeset::LoweringError;

/// PDF ビルド時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub(super) enum BuildPdfError {
  /// テキストファイルの読み込みエラー
  #[error("テキストファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(build::read_text_file),
    help(
      "ファイルのパスと読み取り権限を確認してください。ファイルが UTF-8 でエンコードされていることも確認してください。"
    )
  )]
  ReadTextFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// 複数ソースのパース・評価エラー
  ///
  /// `#[related]` で全エラーをまとめて表示する。
  #[error("複数のソースファイルでエラーが発生しました。")]
  #[diagnostic(code(build::multiple_source_errors))]
  MultipleSourceErrors {
    /// ソースファイルごとのパース・評価エラー
    #[related]
    errors: Vec<ParseSourceError>,
  },

  /// 文献引用の CSL 整形エラー
  #[error("文献引用の整形に失敗しました。")]
  #[diagnostic(code(build::citation))]
  Citation {
    /// 元の citation エラー
    #[source]
    #[diagnostic_source]
    source: CitationError,
  },

  /// 帰属ソースを特定できる lowering エラー
  ///
  /// `NamedSource` を同梱し、該当箇所を診断へ表示する。
  #[error("ドキュメントのレイアウト変換に失敗しました。")]
  #[diagnostic(code(build::lowering))]
  Lowering {
    /// エラーが帰属するソースファイルの名前と内容（診断スニペット用）
    #[source_code]
    src: NamedSource<String>,
    /// 元の lowering エラー
    #[source]
    #[diagnostic_source]
    source: LoweringError,
  },

  /// 特定ソースに帰属できない lowering エラー
  #[error("ドキュメントのレイアウト変換に失敗しました（帰属元ソース不明）。")]
  #[diagnostic(code(build::lowering_internal))]
  LoweringInternal {
    /// 元の lowering エラー
    #[source]
    #[diagnostic_source]
    source: LoweringError,
  },

  /// PDF ファイルの書き込みエラー
  #[error("PDF ファイルの保存に失敗しました: {path}")]
  #[diagnostic(code(build::write_pdf), help("出力ディレクトリが存在し、書き込み権限があることを確認してください。"))]
  WritePdf {
    /// 出力パス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// config と style の横断バリデーションエラー
  #[error("ページレイアウトの検証に失敗しました。")]
  #[diagnostic(code(build::layout))]
  Layout {
    /// 元の横断バリデーションエラー
    #[source]
    #[diagnostic_source]
    source: LayoutValidationError,
  },

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
    /// 元の I/O エラー。
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
    /// 元の `pdf_gen` デコードエラー。
    #[source]
    #[diagnostic_source]
    source: pdf_gen::PdfGenError,
  },

  /// 画像の自然寸法が不正です（縦横比を算出できません）。
  #[error("画像の自然寸法が不正です: {path} (width={width}, height={height})")]
  #[diagnostic(
    code(build::image::invalid_natural_size),
    help("画像ファイルが破損していないか、または width / height を明示指定してください。")
  )]
  InvalidImageNaturalSize {
    /// 画像ファイルのパス。
    path: AssetId,
    /// 自然幅（ラスタはピクセル、SVG は pt）。
    width: f32,
    /// 自然高さ（ラスタはピクセル、SVG は pt）。
    height: f32,
  },

  /// `resolve_images` が `ImageResources` にない画像を参照しました。
  #[error("ImageResources に存在しない画像パスです（内部エラー）: {path}")]
  #[diagnostic(
    code(build::image::not_in_manifest),
    help("ImageManifest の収集ロジックに不具合があります。issue を報告してください。")
  )]
  ImageNotInManifest {
    /// 画像ファイルのパス。
    path: AssetId,
  },
}
