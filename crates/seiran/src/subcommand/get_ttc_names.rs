//! TTC ファイル内の OpenType name レコードを表示するサブコマンド

use std::{fs, path::Path};

use miette::{Diagnostic, IntoDiagnostic};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;
use tracing::info;

/// TTC ファイル情報取得時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum TtcNamesError {
  /// TTC ファイルの読み込みに失敗した場合
  #[error("TTC ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(ttc_names::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// TTC 内の全フォントについて name レコードを標準出力へ表示する。
///
/// # Errors
///
/// ファイルの読み込み、フォントまたは name テーブルの解析に失敗した場合にエラーを返す。
pub(crate) fn get_ttc_names(file_path: &Path) -> miette::Result<()> {
  info!(ttc_path = %file_path.display(), "TTC ファイルを読み込みます");

  let data = fs::read(file_path).map_err(|source| {
    return TtcNamesError::ReadFile {
      path: file_path.display().to_string(),
      source,
    };
  })?;

  let fonts = FontRef::fonts(&data);
  for (index, font) in fonts.enumerate() {
    let font = font.into_diagnostic()?;
    let names = font.name().into_diagnostic()?;

    for name_record in names.name_record() {
      let platform_id = name_record.platform_id();
      let encording_id = name_record.encoding_id();
      let language_id = name_record.language_id();
      let name_id = name_record.name_id();

      let name = name_record.string(names.string_data());

      println!(
        "Font Index {index}: Platform ID {platform_id:?}, Encoding ID {encording_id:?}, Language ID {language_id:?}, Name ID {name_id:?}: {name:?}",
      );
    }
  }

  return Ok(());
}
