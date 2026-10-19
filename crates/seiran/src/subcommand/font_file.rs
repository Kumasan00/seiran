//! フォント調査の前段 — フォントファイルの読込と、`--font-index` による face の選択

use std::{fs, io, path::Path};

use miette::Diagnostic;
use read_fonts::{FontRef, ReadError};
use thiserror::Error;

/// フォントファイルを読めなかったときのエラー。
#[derive(Debug, Error, Diagnostic)]
#[error("フォントファイルの読み込みに失敗しました: {path}")]
#[diagnostic(code(cli::font_file::read_file), help("フォントファイルのパスと読み取り権限を確認してください。"))]
pub(super) struct ReadFontFileError {
  /// ファイルパス
  path: String,
  /// 元の I/O エラー
  #[source]
  source: io::Error,
}

/// ファイルから指定インデックスの face を選べなかったときのエラー。
#[derive(Debug, Error, Diagnostic)]
#[error("インデックス {font_index} のフォント解析に失敗しました: {path}")]
#[diagnostic(
  code(cli::font_file::font_parse),
  help(
    "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は --font-index を確認してください。"
  )
)]
pub(super) struct SelectFaceError {
  /// ファイルパス
  path: String,
  /// フォントインデックス（TTC の場合はコレクション内の位置）
  font_index: u32,
  /// 元の解析エラー
  #[source]
  source: ReadError,
}

/// フォントファイルを読む。
///
/// # Errors
///
/// ファイルを読めなかったとき [`ReadFontFileError`] を返す。
pub(super) fn read(path: &Path) -> Result<Vec<u8>, ReadFontFileError> {
  return fs::read(path).map_err(|source| {
    return ReadFontFileError {
      path: path.display().to_string(),
      source,
    };
  });
}

/// 読み込んだバイト列から `font_index` 番目の face を選ぶ（単体フォントは 0 だけが有効）。
///
/// # Errors
///
/// バイト列がフォントとして解析できない、またはインデックスがコレクションの範囲外のとき
/// [`SelectFaceError`] を返す。
pub(super) fn select_face<'a>(bytes: &'a [u8], font_index: u32, path: &Path) -> Result<FontRef<'a>, SelectFaceError> {
  return FontRef::from_index(bytes, font_index).map_err(|source| {
    return SelectFaceError {
      path: path.display().to_string(),
      font_index,
      source,
    };
  });
}

#[cfg(test)]
mod tests {
  use std::{error::Error, path::Path};

  use miette::Diagnostic;

  use super::{read, select_face};

  /// 診断の code・主メッセージ・help を文字列の組にする。
  fn rendered(error: &impl Diagnostic) -> (String, String, String) {
    let code = error.code().expect("前段の失敗は code を持つ").to_string();
    let help = error.help().expect("前段の失敗は help を持つ").to_string();
    return (code, error.to_string(), help);
  }

  #[test]
  fn read_failure_has_one_code_and_wording_for_every_subcommand() {
    let error = read(Path::new("no-such-dir/missing")).expect_err("存在しないパスは読めない");

    assert_eq!(
      rendered(&error),
      (
        "cli::font_file::read_file".to_owned(),
        "フォントファイルの読み込みに失敗しました: no-such-dir/missing".to_owned(),
        "フォントファイルのパスと読み取り権限を確認してください。".to_owned()
      )
    );
    assert!(error.source().is_some(), "OS エラーは cause に残す");
  }

  #[test]
  fn face_selection_failure_has_one_code_and_wording_for_every_subcommand() {
    let Err(error) = select_face(b"not a font", 3, Path::new("notes.txt")) else {
      panic!("フォントでないバイト列から face は選べない");
    };

    assert_eq!(
      rendered(&error),
      (
        "cli::font_file::font_parse".to_owned(),
        "インデックス 3 のフォント解析に失敗しました: notes.txt".to_owned(),
        "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は --font-index を確認してください。".to_owned()
      )
    );
    assert!(error.source().is_some(), "解析エラーは cause に残す");
  }
}
