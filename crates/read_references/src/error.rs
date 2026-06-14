//! 参照定義ファイル読み込み時のエラー型。
//!
//! 読み込み・TOML / JSON 解析・拡張子判定の失敗を表す [`ReadReferencesError`] を定義する。著者名の
//! 排他性違反や空・重複 ID は、デシリアライズ時点で fail-fast に検出され
//! [`ReadReferencesError::ParseToml`] / [`ReadReferencesError::ParseJson`] として報告される。

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
}
