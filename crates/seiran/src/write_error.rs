//! PDF ファイル保存時のエラー型（`compile` の外側、CLI だけが持つ関心事）

use miette::Diagnostic;
use thiserror::Error;

/// PDF ファイルの保存時に起きるエラー型。
///
/// `compile` は保存を行わないため、出力ディレクトリの作成・書き込みに関するエラーは
/// `seiran_compiler::CompileFailure` ではなくこの型が持つ（CLI だけが必要とする関心事のため）。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum WriteError {
  /// 出力ディレクトリの作成エラー
  #[error("出力ディレクトリを作成できませんでした: {path}")]
  #[diagnostic(
    code(cli::create_output_dir),
    help("親ディレクトリが存在し、書き込み権限があることを確認してください。")
  )]
  CreateOutputDir {
    /// 出力ディレクトリのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// PDF ファイルの書き込みエラー
  #[error("PDF ファイルの保存に失敗しました: {path}")]
  #[diagnostic(code(cli::write_pdf), help("出力ディレクトリが存在し、書き込み権限があることを確認してください。"))]
  WritePdf {
    /// 出力パス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}
