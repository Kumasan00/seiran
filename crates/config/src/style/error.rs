//! [`crate::style::read_style`] が返すエラー型の定義。

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

use crate::project_source::SourceReadError;

/// スタイル設定ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ReadStyleError {
  /// スタイル設定ファイルの読み込み失敗（I/O エラー）
  #[error("スタイル設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(style::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー
    #[source]
    #[diagnostic_source]
    source: SourceReadError,
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
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("スタイル設定のバリデーションに失敗しました。")]
  #[diagnostic(code(style::multiple_validation_errors))]
  MultipleValidationErrors {
    /// 検証で検出されたすべてのエラー
    #[related]
    errors: Vec<StyleValidationError>,
  },
}

/// スタイル設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum StyleValidationError {
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
    code(style::validation::csl_path_resolution),
    help("style.toml の [reference].csl_path が指すファイルが存在し、読み取り権限があることを確認してください。")
  )]
  CslPathResolution {
    /// 見つからなかったパス
    path: String,
  },

  /// `locale_path`（CSL ロケールファイル）が見つからない。
  #[error("CSL ロケールファイルが見つかりません: {path}")]
  #[diagnostic(
    code(style::validation::locale_path_resolution),
    help("style.toml の [reference].locale_path が指すファイルが存在し、読み取り権限があることを確認してください。")
  )]
  LocalePathResolution {
    /// 見つからなかったパス
    path: String,
  },
}
