//! PDF ビルドエラー型の定義

use miette::{Diagnostic, LabeledSpan, NamedSource};
use thiserror::Error;

use crate::{
  config::LayoutValidationError,
  frontend::ParseSourceError,
  semantics::{CitationFormatError, CitationStyleError, SemanticError},
};

/// [`crate::frontend::ParseSourceError`] に、`SourceDb` から引いた [`NamedSource`] を添えて表示可能にする。
///
/// `ParseSourceError` は `SourceId` だけを持ち、ソース本文を持たない（本文は
/// `snapshot::SourceDb` が一元管理する）。code / message / help / label / related は
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

/// 1 ソース内の未定義引用キーを、`SourceDb` から引いた [`NamedSource`] を添えて表示可能にする。
///
/// [`crate::semantics::CitationSemanticError::UnknownCitationKeys`] は `SourceId` だけを持ち、
/// ソース本文を持たない（本文は `snapshot::SourceDb` が一元管理する）。同じソース内に未定義キーが
/// 複数箇所あれば `labels` にまとめて積む（`labels` の要素ごとに独自のラベル文言を持つため、
/// `#[label(collection)]` に静的な文言は付けない）。この型自身が診断メッセージ・code・help を
/// 持つ独立した診断であり、`AttributedParseError` のように内側のエラー型へ委譲する必要が無いため、
/// 通常の `thiserror::Error` + `#[derive(Diagnostic)]` で書ける。
///
/// `#[error(...)]` / `help(...)` の文言は [`crate::semantics::CitationSemanticError::UnknownCitationKeys`]
/// と意図的に同じ文面を**再掲**している（委譲ではない — `wrap_citation_semantic_error` が
/// `UnknownCitationKeys` を分解してこの型を組み立てるため、実際に描画されるのはこちらのコピーだけで、
/// 元の `CitationSemanticError` 側の code / help は生成物の Report には現れない）。文言を変えるときは
/// 両方を揃えて更新すること。
#[derive(Debug, Error, Diagnostic)]
#[error("未定義の引用キーがあります")]
#[diagnostic(
  code(build::citation::unknown_citation_key),
  help("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください")
)]
pub(super) struct AttributedCitationError {
  /// `SourceDb` から引いたソース名・本文（`source_code` の供給元）
  #[source_code]
  src: NamedSource<String>,
  /// このソース内で見つかった未定義引用キーの箇所（`\cite{...}` ごとに 1 ラベル）
  #[label(collection)]
  labels: Vec<LabeledSpan>,
}

impl AttributedCitationError {
  /// `SourceDb` から引いた `NamedSource` と、そのソース内の未定義キーラベル列を束ねる
  pub(super) fn new(src: NamedSource<String>, labels: Vec<LabeledSpan>) -> Self {
    return AttributedCitationError { src, labels };
  }
}

/// `compile` の内部段（読込・パース・意味解決・組版）で起きるエラー型。
///
/// PDF の保存（`WritePdf` 相当）は `compile` の責務ではなくなったため、ここには含まない
/// （呼び出し元である CLI 側が保存専用のエラー型を別途持つ）。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum CompileError {
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
    /// 元の読込エラー
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

  /// CSL スタイル（`.csl`）・ロケールの読込・解析エラー
  #[error("文献引用の CSL スタイルを読み込めませんでした。")]
  #[diagnostic(code(build::citation::style))]
  CitationStyle {
    /// 元の CSL スタイル読込エラー
    #[source]
    #[diagnostic_source]
    source: CitationStyleError,
  },

  /// 文献引用の CSL 整形（表示の生成）エラー
  #[error("文献引用の整形に失敗しました。")]
  #[diagnostic(code(build::citation::format))]
  CitationFormat {
    /// 元の CSL 整形エラー
    #[source]
    #[diagnostic_source]
    source: CitationFormatError,
  },

  /// `\cite{...}` の未定義引用キー（ソースごとに集約）
  ///
  /// `#[related]` で全ソースの診断をまとめて表示する（`MultipleSourceErrors` と同じ
  /// 「1 ソース内は 1 件のラベル付き診断へまとめ、複数ソースは `#[related]` で束ねる」構成）。
  #[error("複数の引用箇所で未定義の引用キーがあります。")]
  #[diagnostic(code(build::multiple_citation_errors))]
  MultipleCitationErrors {
    /// ソースごとの未定義引用キー診断（`SourceDb` から引いた本文を添えたもの）
    #[related]
    errors: Vec<AttributedCitationError>,
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
    source: SemanticError,
  },

  /// カレントディレクトリの取得失敗
  #[error("カレントディレクトリを取得できませんでした。")]
  #[diagnostic(code(build::current_dir), help("プロセスの作業ディレクトリが有効か確認してください。"))]
  CurrentDir {
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

  /// 組版パス（画像資源の解決を含む）のエラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Typeset(#[from] crate::typeset::TypesetError),
}
