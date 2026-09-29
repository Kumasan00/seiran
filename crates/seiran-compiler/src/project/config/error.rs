//! [`crate::project::config::load`] が返すエラー型と警告型の定義。

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

use crate::project::{FontType, InFile, SourceReadError};

/// 設定ファイル読み込みで発生するすべてのエラー。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ReadConfigError {
  /// 設定ファイルの読み込み失敗
  #[error("設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(project::config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// 読み込みに失敗した設定ファイルのパス
    path: String,
    #[source]
    /// 元の読み込みエラー（低水準 cause）
    source: SourceReadError,
  },
  /// TOML 解析失敗
  #[error("設定ファイルの TOML 解析に失敗しました")]
  #[diagnostic(
    code(project::config::parse_toml),
    help("TOML の構文とキー名を確認してください。使えないキー（廃止されたキーを含む）は削除してください。")
  )]
  ParseToml {
    #[source_code]
    /// エラー位置を示すためのソース全文
    src: NamedSource<String>,
    #[label("ここ")]
    /// エラー箇所のソース内スパン
    span: SourceSpan,
    #[source]
    /// 元の TOML パースエラー
    source: toml::de::Error,
  },
  /// 値検証の違反 1 件（実際に読んだ config ファイルのパスを添える）
  ///
  /// 複数の違反は `Failures<ReadConfigError>` の別要素として並ぶ。段名だけを表す集約
  /// バリアントは持たない — ユーザーが最初に読むのは
  /// 「どのフィールドをどう直すか」であるべきで、「複数のバリデーションエラー」ではない。
  /// パスは `-c` で任意の名前を付けた設定ファイルでも分かるように添える。
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] InFile<ConfigValidationError>),
}

/// 設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ConfigValidationError {
  /// 設定値の不正（garde の値検証・フォントの言語制約・タグと書字方向の変換で検出）
  #[error("'{path}': {message}")]
  #[diagnostic(
    code(project::config::validation::field),
    help("config.toml の該当フィールドの値を確認してください。")
  )]
  Field {
    /// 不正な値を持つフィールドの TOML パス（例: `pdf.width`）
    path: String,
    /// 違反の内容
    message: String,
  },
  /// フォントパスが見つからない
  #[error("フォントファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::font_path),
    help("フォントファイルが存在し、読み取り権限があることを確認してください。")
  )]
  FontPathResolution {
    /// 対象のフォント種別
    font_type: FontType,
    /// 見つからなかったフォントファイルのパス
    path: String,
  },
  /// スタイル設定ファイルが見つからない
  #[error("スタイル設定ファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::style_path),
    help("スタイル設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  StylePathResolution {
    /// 見つからなかったスタイル設定ファイルのパス
    path: String,
  },
  /// 参照設定ファイルが見つからない
  #[error("参照設定ファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::references_path),
    help("参照設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  ReferencesPathResolution {
    /// 見つからなかった参照設定ファイルのパス
    path: String,
  },
  /// ソースファイルが見つからない
  #[error("ソースファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::source_path),
    help("`sources` に列挙したファイルが存在し、読み取り権限があることを確認してください。")
  )]
  SourcePathResolution {
    /// 見つからなかったソースファイルのパス
    path: String,
  },
}

/// config.toml の警告（読み込みは成功するが、ユーザーが直したほうがよい問題）。
#[derive(Debug, Clone, Error, Diagnostic)]
pub(crate) enum ConfigWarning {
  /// `sources` のファイル拡張子が `.sei` ではない。
  #[error("ソースファイルの拡張子が `.sei` ではありません: {path}")]
  #[diagnostic(
    code(project::config::source_extension),
    severity(Warning),
    help("Seiran のソースファイルには拡張子 `.sei` を使ってください。")
  )]
  SourceExtension {
    /// config.toml に書かれたままのソースパス
    path: String,
  },
}
