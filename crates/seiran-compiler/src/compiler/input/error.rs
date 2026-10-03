//! 入力読込で発生するエラー型の定義

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  project::{ProjectSourceError, ReadFontError, config::ReadConfigError},
  semantics::ReadReferencesError,
  style::ReadStyleError,
  typeset::GeometryValidationError,
};

/// `compile` の入力読込（設定・スタイル・横断検証・文献・フォント・ソース）で起きるエラー型。
///
/// **段の名前を足す wrapper にはしない** — 内側が独立した診断（`Diagnostic`）を持つものは
/// `transparent` でそのまま委譲し、ユーザーが最初に読むメッセージが常に修正可能な leaf に
/// なるようにする。自前のバリアントを持つのは、内側が `ProjectSourceError` で診断を持たず、
/// パス入りのメッセージと help をこの型自身が与える `ReadTextFile` 1 つだけ。
#[derive(Debug, Error, Diagnostic)]
pub(in crate::compiler) enum InputError {
  /// テキストファイルの読み込みエラー
  #[error("テキストファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(compiler::read_text_file),
    help(
      "ファイルのパスと読み取り権限を確認してください。ファイルが UTF-8 でエンコードされていることも確認してください。"
    )
  )]
  ReadTextFile {
    /// ファイルパス
    path: String,
    /// 元の読込エラー（低水準 cause）
    #[source]
    source: ProjectSourceError,
  },

  /// config.toml の読込・検証エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Config(#[from] ReadConfigError),

  /// style.toml の読込・検証エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Style(#[from] ReadStyleError),

  /// config と style の横断バリデーションエラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Geometry(#[from] GeometryValidationError),

  /// 文献データ（references.toml / .json）の読込エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  References(#[from] ReadReferencesError),

  /// フォントファイルの読込エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Font(#[from] ReadFontError),
}
