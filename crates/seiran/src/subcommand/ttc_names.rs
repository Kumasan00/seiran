//! TTC ファイル内の OpenType name レコードを表示するサブコマンド

use std::{fs, io::Write, path::Path};

use miette::Diagnostic;
use read_fonts::{FileRef, ReadError, TableProvider};
use thiserror::Error;
use tracing::info;

use crate::subcommand::listing;

/// TTC ファイル情報取得時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum TtcNamesError {
  /// ファイルの読み込みに失敗した場合
  #[error("TTC ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(cli::ttc_names::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// ファイルがフォントとしてもフォントコレクションとしても解析できない場合
  #[error("フォントファイルとして解析できませんでした: {path}")]
  #[diagnostic(
    code(cli::ttc_names::file_parse),
    help("ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。")
  )]
  FileParse {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },

  /// コレクション内のフォントを解析できない場合
  #[error("インデックス {font_index} のフォント解析に失敗しました: {path}")]
  #[diagnostic(
    code(cli::ttc_names::font_parse),
    help("フォントコレクションのヘッダかテーブルディレクトリが破損している可能性があります。")
  )]
  FontParse {
    /// ファイルパス
    path: String,
    /// コレクション内のフォントインデックス
    font_index: usize,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },

  /// name テーブルを読めない場合
  #[error("インデックス {font_index} のフォントの name テーブルを読めませんでした: {path}")]
  #[diagnostic(
    code(cli::ttc_names::name),
    help("name テーブルが欠落しているか破損しています。フォントファイルを検証してください。")
  )]
  Name {
    /// ファイルパス
    path: String,
    /// コレクション内のフォントインデックス
    font_index: usize,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },
}

/// ファイル内の全フォントについて name レコードの一覧を `out` へ書く。
///
/// # Errors
///
/// ファイルの読み込み、フォントまたは name テーブルの解析、一覧の書き込み（受け手の終了を除く）に
/// 失敗した場合にエラーを返す。
pub(crate) fn ttc_names(file_path: &Path, out: &mut impl Write) -> miette::Result<()> {
  let data = fs::read(file_path).map_err(|source| {
    return TtcNamesError::ReadFile {
      path: file_path.display().to_string(),
      source,
    };
  })?;
  info!(ttc_path = %file_path.display(), "TTC ファイルを読込");

  let lines = listing_lines(&data, file_path)?;
  listing::emit(&lines, out)?;
  return Ok(());
}

/// ファイル内の全フォントの name レコードを 1 行ずつ整形する。
///
/// `FontRef::fonts` ではなく `FileRef::new` を直接呼ぶ — `FontRef::fonts` は解析できないファイルを
/// 「フォント 0 件」に畳むので、フォントでないファイルが空の一覧で成功してしまう。
/// 個々の name 文字列を読めなかったレコードは `Err(..)` を表示したまま一覧に残す（部分結果の
/// マーカー）。フォント自体・name テーブル自体を読めないときは一覧を出さずに失敗する。
fn listing_lines(data: &[u8], file_path: &Path) -> Result<Vec<String>, TtcNamesError> {
  let path = || return file_path.display().to_string();
  let file = FileRef::new(data).map_err(|source| {
    return TtcNamesError::FileParse {
      path: path(),
      source,
    };
  })?;

  let mut lines = Vec::new();
  for (font_index, font) in file.fonts().enumerate() {
    let font = font.map_err(|source| {
      return TtcNamesError::FontParse {
        path: path(),
        font_index,
        source,
      };
    })?;
    let names = font.name().map_err(|source| {
      return TtcNamesError::Name {
        path: path(),
        font_index,
        source,
      };
    })?;

    for name_record in names.name_record() {
      let platform_id = name_record.platform_id();
      let encoding_id = name_record.encoding_id();
      let language_id = name_record.language_id();
      let name_id = name_record.name_id();
      let name = name_record.string(names.string_data());

      lines.push(format!(
        "Font Index {font_index}: Platform ID {platform_id:?}, Encoding ID {encoding_id:?}, Language ID {language_id:?}, Name ID {name_id:?}: {name:?}",
      ));
    }
  }
  return Ok(lines);
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::{TtcNamesError, listing_lines};

  #[test]
  fn non_font_bytes_are_a_parse_error() {
    let error = listing_lines(b"[package]\nname = \"seiran\"\n", Path::new("Cargo.toml"))
      .expect_err("フォントでないファイルは失敗として報告する");

    assert!(
      matches!(&error, TtcNamesError::FileParse { path, .. } if path == "Cargo.toml"),
      "対象パス付きの解析失敗になる: {error:?}"
    );
  }

  #[test]
  fn empty_file_is_a_parse_error() {
    let error = listing_lines(b"", Path::new("empty.ttc")).expect_err("空のファイルは失敗として報告する");

    assert!(matches!(error, TtcNamesError::FileParse { .. }), "解析失敗になる: {error:?}");
  }
}
