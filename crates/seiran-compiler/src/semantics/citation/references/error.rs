//! 参照定義ファイル読み込み時のエラー型。

use miette::Diagnostic;
use thiserror::Error;

use crate::project::SourceReadError;

/// 参照定義ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ReadReferencesError {
  /// 参照定義ファイルの読み込みに失敗した場合
  #[error("参照定義ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(semantics::citation::references::read_file),
    help("ファイルのパスと読み取り権限を確認してください。")
  )]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー（低水準 cause）
    #[source]
    source: SourceReadError,
  },
  /// TOML 解析に失敗した場合
  #[error("参照定義ファイルの TOML 解析に失敗しました: {path}")]
  #[diagnostic(code(semantics::citation::references::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: toml::de::Error,
  },
  /// JSON 解析に失敗した場合
  #[error("参照定義ファイルの JSON 解析に失敗しました: {path}")]
  #[diagnostic(code(semantics::citation::references::parse_json), help("JSON の構文を確認してください。"))]
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
    code(semantics::citation::references::unsupported_extension),
    help("拡張子を `.toml` または `.json` のいずれかに変更してください。")
  )]
  UnsupportedExtension {
    /// ファイルパス
    path: String,
  },
}
