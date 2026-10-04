//! [`crate::style::load`] が返すエラー型の定義。

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

use crate::project::{InFile, ProjectSourceError};

/// スタイル設定ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ReadStyleError {
  /// スタイル設定ファイルの読み込み失敗（I/O エラー）
  #[error("スタイル設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(style::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー（低水準 cause）
    #[source]
    source: ProjectSourceError,
  },
  /// TOML の構文・型・未知キー等のパース失敗
  #[error("スタイル設定の TOML 解析に失敗しました")]
  #[diagnostic(code(style::parse_toml), help("TOML の構文とフィールドの型を確認してください。"))]
  ParseToml {
    /// ソース名付きの元テキスト（`#[label]` レンダリング用）
    #[source_code]
    src: NamedSource<String>,
    /// ソース上のスパン。`toml::de::Error` から取得。
    #[label("ここ")]
    span: SourceSpan,
    /// 元の toml エラー（チェーン表示で根本原因を補足）
    #[source]
    source: toml::de::Error,
  },
  /// 値検証の違反 1 件（実際に読んだ style ファイルのパスを添える）
  ///
  /// 複数の違反は `Failures<ReadStyleError>` の別要素として並ぶ。
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] InFile<StyleValidationError>),
}

/// スタイル設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum StyleValidationError {
  /// garde が検出したスタイル設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(style::validation::field), help("style.toml の該当フィールドの値を確認してください。"))]
  Field {
    /// 不正なフィールドのパス（例: `font_size`, `heading.section.font_size`）
    path: String,
    /// 不正の内容
    message: String,
  },

  /// `csl_path`（CSL スタイルファイル）が見つからない。
  #[error("CSL スタイルファイルが見つかりません: {path}")]
  #[diagnostic(
    code(style::validation::csl_file_not_found),
    help("style.toml の [reference].csl_path が指すファイルが存在し、読み取り権限があることを確認してください。")
  )]
  CslFileNotFound {
    /// 見つからなかったパス
    path: String,
  },

  /// `locale_path`（CSL ロケールファイル）が見つからない。
  #[error("CSL ロケールファイルが見つかりません: {path}")]
  #[diagnostic(
    code(style::validation::locale_file_not_found),
    help("style.toml の [reference].locale_path が指すファイルが存在し、読み取り権限があることを確認してください。")
  )]
  LocaleFileNotFound {
    /// 見つからなかったパス
    path: String,
  },

  /// `[toc].alignment` を `"left"` 以外にしたまま `[toc].show_page_numbers = true` にしている。
  ///
  /// ページ番号を出す目次の行は内容によらず版面の右端まで伸びるので、揃えはどの内容に対しても出力を変えない。
  #[error("[toc].alignment が \"left\" 以外のときは [toc].show_page_numbers = false が必要です")]
  #[diagnostic(
    code(style::validation::toc_alignment_with_page_numbers),
    help(
      "ページ番号を出す目次の行は版面の右端まで伸びるため、揃えを変えても出力は変わりません。揃えを使うなら \
       show_page_numbers = false にし、ページ番号を出すなら alignment を外してください。"
    )
  )]
  TocAlignmentWithPageNumbers,
}
