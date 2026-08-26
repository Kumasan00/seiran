//! 入力読込で発生するエラー型の定義

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  project::{FontReadError, SourceReadError, config::ReadConfigError},
  semantics::ReadReferencesError,
  style::ReadStyleError,
  typeset::LayoutValidationError,
};

/// `compile` の入力読込（設定・スタイル・横断検証・文献・フォント・ソース）で起きるエラー型。
///
/// **段の名前を足す wrapper にはしない** — 内側が独立した診断（`Diagnostic`）を持つものは
/// `transparent` でそのまま委譲し、ユーザーが最初に読むメッセージが常に修正可能な leaf に
/// なるようにする。自前のバリアントを持つのは、内側が `std::io::Error` で診断を持たず、
/// パス入りのメッセージと help をこの型自身が与える 2 つだけ。
///
/// PDF の保存は `compile` の責務ではないため、ここには含まない
/// （呼び出し元である CLI 側が保存専用のエラー型を別途持つ）。
///
/// 可視性が crate 内で唯一の `pub(in ...)` 形なのは、この型が `input::load` の `pub(super)` な
/// シグネチャに現れるため — `compiler` module 全体から名指しできないと rustc の
/// `private_interfaces` が落ちる。`compiler` の外に消費者はいないので `pub(crate)` へは広げない。
#[derive(Debug, Error, Diagnostic)]
pub(in crate::compiler) enum CompileError {
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
    source: SourceReadError,
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
  Layout(#[from] LayoutValidationError),

  /// 文献データ（references.toml / .json）の読込エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  References(#[from] ReadReferencesError),

  /// フォントファイルの読込エラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Font(#[from] FontReadError),
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;

  use super::CompileError;
  use crate::typeset::LayoutValidationError;

  #[test]
  fn layout_error_keeps_the_inner_leaf_code() {
    // Arrange — 横断検証の leaf エラー（段名だけの wrapper を挟まないことの回帰）
    let error = LayoutValidationError::InvalidColumnWidth {
      text_width: 100.0,
      num_columns: 2,
      column_gap: 200.0,
    };

    // Act
    let error = CompileError::from(error);

    // Assert — `compiler::layout` ではなく内側の leaf の code が出るはず
    assert_eq!(
      error.code().expect("code を持つはず").to_string(),
      "typeset::geometry::invalid_columns",
      "段名だけの wrapper が主診断になってはいけない"
    );
  }
}
