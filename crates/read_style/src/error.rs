//! [`crate::read_style`] が返すエラー型の定義。

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

/// スタイル設定ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ReadStyleError {
  /// スタイル設定ファイルの読み込み失敗（I/O エラー）
  #[error("スタイル設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(style::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// スタイル設定の TOML 解析または既定値とのマージに失敗した場合
  ///
  /// TOML の構文エラーに加え、フィールドの型不一致や未知のキー、デフォルト値との
  /// マージ失敗もこのバリアントに含まれます。figment が報告したエラーチェーンを
  /// [`ParseTomlError`] のベクタに展開し、`#[related]` 経由で一度にすべて表示します。
  /// 各 [`ParseTomlError`] が自前で [`NamedSource`] を保持し、その `name` がファイル
  /// パスを兼ねるため、親バリアントには冗長な `path` フィールドを持たせません。
  #[error("スタイル設定の解析またはデフォルト値とのマージに失敗しました。")]
  #[diagnostic(code(style::parse_toml))]
  ParseToml {
    /// figment のエラーチェーンを展開した個別エラー群
    #[related]
    errors: Vec<ParseTomlError>,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("スタイル設定のバリデーションに失敗しました。")]
  #[diagnostic(code(style::multiple_validation_errors))]
  MultipleValidationErrors {
    /// 検証で検出されたすべてのエラー
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// [`ReadStyleError::ParseToml`] の内側エラー。
///
/// figment のエラーチェーンの 1 要素をラップします。`#[related]` の子要素は親の
/// `#[source_code]` を継承しないため、各エラーが自前で [`NamedSource`] を保持します
/// （[`NamedSource`] は `Clone` 実装を持つので複製コストは問題になりません）。
/// 表示メッセージは日本語でキーパスのみを示し、figment の英語メッセージは
/// `#[source]` チェーン経由でレンダリングして二重表示を避けます。
/// figment のキーパスから推定したソース位置を `span` に持ち、`#[label]` で該当箇所を
/// ハイライト表示します。span を計算できなかった場合は `0..0` を持つため、ラベルは
/// 表示されませんがエラーメッセージ自体には影響しません。
#[derive(Debug, Error, Diagnostic)]
#[error("キー '{key_path}' の解析に失敗しました")]
#[diagnostic(code(style::parse_toml::field), help("TOML の構文とフィールドの型を確認してください。"))]
pub struct ParseTomlError {
  /// figment が報告したキーパス（例: `chapter.font_size`）。トップレベルなら `"(root)"`
  pub(crate) key_path: String,
  /// ソース名付きの元テキスト（`#[label]` レンダリング用）
  #[source_code]
  pub(crate) src: NamedSource<String>,
  /// ソース上のスパン（推定）。`0..0` の場合はラベル非表示
  #[label("ここ")]
  pub(crate) span: SourceSpan,
  /// 元の figment エラー（チェーン表示で根本原因を補足）
  #[source]
  pub(crate) source: figment2::Error,
}

/// スタイル設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// garde が検出したスタイル設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(style::validation::field), help("style.toml の該当フィールドの値を確認してください。"))]
  Field {
    /// 不正なフィールドのパス（例: `font_size`, `part.font_size`）
    path: String,
    /// 不正の内容
    message: String,
  },
}
