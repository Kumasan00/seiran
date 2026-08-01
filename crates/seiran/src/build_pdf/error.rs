//! PDF ビルドエラー型の定義

use citation::CitationError;
use config::LayoutValidationError;
use frontend::ParseSourceError;
use miette::{Diagnostic, NamedSource};
use model::AssetId;
use pdf_gen::PdfGenError;
use resolve::ResolveError;
use thiserror::Error;

/// [`frontend::ParseSourceError`] に、`SourceDb` から引いた [`NamedSource`] を添えて表示可能にする。
///
/// `ParseSourceError` は `SourceId` だけを持ち、ソース本文を持たない（本文は
/// `project::SourceDb` が一元管理する）。code / message / help / label / related は
/// すべて内側の `ParseSourceError` へ委譲し、`source_code` だけをここで補う
/// （`#[diagnostic(transparent)]` は `source_code` も内側へ委譲してしまうため使えず、手書きする）。
#[derive(Debug)]
pub(super) struct AttributedParseError {
  /// `SourceDb` から引いたソース名・本文（`source_code` の供給元）
  named_source: NamedSource<String>,
  /// 内側のパース・評価エラー（`SourceId` のみを持ち本文は持たない）
  inner: ParseSourceError,
}

impl AttributedParseError {
  /// `SourceDb` から引いた `NamedSource` と内側の `ParseSourceError` を束ねる
  pub(super) fn new(named_source: NamedSource<String>, inner: ParseSourceError) -> Self {
    return AttributedParseError {
      named_source,
      inner,
    };
  }
}

impl std::fmt::Display for AttributedParseError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return self.inner.fmt(f); }
}

impl std::error::Error for AttributedParseError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return std::error::Error::source(&self.inner); }
}

impl Diagnostic for AttributedParseError {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.inner.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.inner.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.inner.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.inner.url(); }

  fn source_code(&self) -> Option<&dyn miette::SourceCode> { return Some(&self.named_source); }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> { return self.inner.labels(); }

  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> { return self.inner.related(); }

  fn diagnostic_source(&self) -> Option<&dyn Diagnostic> { return self.inner.diagnostic_source(); }
}

/// compiler の不変条件違反（ユーザー入力に起因しない内部バグ）。
///
/// `BuildPdfError` の他バリアントが表す「設定・入力の誤り」とは型を分け、
/// 呼び出し元が両者を混同して同じ助言文で案内しないようにする。
#[derive(Debug, Error, Diagnostic)]
#[error("内部エラー: {message}")]
#[diagnostic(code(build::internal_bug), help("再現手順とともに issue を報告してください。"))]
pub(super) struct CompilerBug {
  /// エラーメッセージ
  message: String,
}

impl CompilerBug {
  /// 新しい `CompilerBug` を構築する
  pub(super) fn new(message: impl Into<String>) -> Self {
    return CompilerBug {
      message: message.into(),
    };
  }
}

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
    /// ソースファイルごとのパース・評価エラー（`SourceDb` から引いた本文を添えたもの）
    #[related]
    errors: Vec<AttributedParseError>,
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

  /// 帰属ソースを特定できる resolve エラー
  ///
  /// `NamedSource` を同梱し、該当箇所を診断へ表示する。
  #[error("ラベル・参照・引用の解決に失敗しました。")]
  #[diagnostic(code(build::resolve))]
  Resolve {
    /// エラーが帰属するソースファイルの名前と内容（診断スニペット用）
    #[source_code]
    src: NamedSource<String>,
    /// 元の resolve エラー
    #[source]
    #[diagnostic_source]
    source: ResolveError,
  },

  /// 特定ソースに帰属できない resolve エラー
  #[error("ラベル・参照・引用の解決に失敗しました（帰属元ソース不明）。")]
  #[diagnostic(code(build::resolve_internal))]
  ResolveInternal {
    /// 元の resolve エラー
    #[source]
    #[diagnostic_source]
    source: ResolveError,
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
    path: AssetId,
    /// 自然幅（ラスタはピクセル、SVG は pt）。
    width: f32,
    /// 自然高さ（ラスタはピクセル、SVG は pt）。
    height: f32,
  },

  /// compiler の不変条件違反（ユーザー向け診断とは別の型・経路）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Bug(#[from] CompilerBug),
}
