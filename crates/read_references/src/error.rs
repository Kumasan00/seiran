//! 参照定義ファイル読み込み時のエラー型。
//!
//! 読み込み・TOML / JSON 解析・拡張子判定の失敗を表す [`ReadReferencesError`] と、値検証の個別違反を
//! 表す [`ValidationError`] を定義する。検証フェーズで検出した複数の違反は
//! [`ReadReferencesError::MultipleValidationErrors`] に集約し、1 度に報告する。

use miette::Diagnostic;
use thiserror::Error;

/// 参照定義ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ReadReferencesError {
  /// 参照定義ファイルの読み込みに失敗した場合
  #[error("参照定義ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(references::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// TOML 解析に失敗した場合
  #[error("参照定義ファイルの TOML 解析に失敗しました: {path}")]
  #[diagnostic(code(references::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: toml::de::Error,
  },
  /// JSON 解析に失敗した場合
  #[error("参照定義ファイルの JSON 解析に失敗しました: {path}")]
  #[diagnostic(code(references::parse_json), help("JSON の構文を確認してください。"))]
  ParseJson {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: serde_json::Error,
  },
  /// サポートされていない拡張子
  #[error("参照定義ファイルの拡張子がサポートされていません: {path}")]
  #[diagnostic(
    code(references::unsupported_extension),
    help("拡張子を `.toml` または `.json` のいずれかに変更してください。")
  )]
  UnsupportedExtension {
    /// ファイルパス
    path: String,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("参照定義のバリデーションに失敗しました。")]
  #[diagnostic(code(references::multiple_validation_errors))]
  MultipleValidationErrors {
    /// 検証で検出されたすべてのエラー
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// 参照定義値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// 著者名（family/literal 排他）の不正、または空 ID などの構造的不正
  #[error("'{path}': {message}")]
  #[diagnostic(
    code(references::validation::field),
    help("references.toml の該当フィールドの値を確認してください。")
  )]
  Field {
    /// 不正なフィールドのパス（例: `references.ref1.author[0]`）
    path: String,
    /// 不正の内容
    message: String,
  },
}
