//! `--log-file` の書き出し先（ログファイルを開く処理と、そこへ書くための sink）
//!
//! ログファイルは「1 回の実行の記録」なので、実行のたびに truncate して開き直す。tracing の layer と
//! ユーザー向け報告（warning 診断・成功サマリ）は同じ [`LogSink`] を共有し、1 本のチャネル越しに書く。

use std::{
  fs::{self, File},
  io::Write,
  path::Path,
};

use miette::Diagnostic;
use thiserror::Error;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::fmt::MakeWriter;

/// ログファイルへの書き出し口。
///
/// tracing の layer へ渡す writer と、warning 診断・成功サマリを直接書く経路の両方が同じチャネルを使う。
/// 経路を 1 本に保つのは、イベントと報告の前後関係を崩さないためと、書き切りの保証を
/// [`WorkerGuard`] 1 つに集約するため。
pub(super) struct LogSink {
  /// ワーカースレッドへ送る writer（layer へ渡すぶんは複製する）
  writer: NonBlocking,
  /// 保持している間だけワーカーが生き、drop 時に書き残しを流し切る guard
  _guard: WorkerGuard,
}

impl LogSink {
  /// `path` を truncate して開き、書き出し口を作る。
  ///
  /// # Errors
  ///
  /// 親ディレクトリを作れない、またはファイルを開けないときに [`LogFileError`] を返す。
  pub(super) fn open(path: &Path) -> Result<Self, LogFileError> {
    let file = open_log_file(path)?;
    // 溢れたイベントを捨てない（`lossy(false)`）。捨ててしまうと「1 件も欠けない記録」にならない。
    let (writer, guard) = NonBlockingBuilder::default().lossy(false).finish(file);
    return Ok(LogSink {
      writer,
      _guard: guard,
    });
  }

  /// tracing の layer へ渡す writer を複製する。
  pub(super) fn writer(&self) -> NonBlocking { return self.writer.clone(); }

  /// ユーザー向け報告 1 件ぶんをファイルへ書く（末尾に改行を足す）。
  ///
  /// 書き込みの失敗は捨てる。ログを残せなかったことを理由に、既に成功した PDF 生成の報告を
  /// 中断させないため。
  pub(super) fn write_block(&self, text: &str) {
    let mut writer = MakeWriter::make_writer(&self.writer);
    let _ = writer.write_all(text.as_bytes());
    let _ = writer.write_all(b"\n");
  }
}

/// ログファイルを truncate して開く。
///
/// 親ディレクトリが無ければ作る — 出力先を掘ってから実行し直す手間を、ログの指定ごときで
/// 掛けさせないため。
fn open_log_file(path: &Path) -> Result<File, LogFileError> {
  if let Some(parent) = parent_to_create(path) {
    fs::create_dir_all(parent).map_err(|source| {
      return LogFileError::CreateLogDir {
        path: parent.display().to_string(),
        source,
      };
    })?;
  }
  return File::create(path).map_err(|source| {
    return LogFileError::Open {
      path: path.display().to_string(),
      source,
    };
  });
}

/// 事前に作る必要のある親ディレクトリを返す。
///
/// カレントディレクトリ直下のパス（`run.log` 等）の `parent()` は空パスを返すので、作成対象から外す。
fn parent_to_create(path: &Path) -> Option<&Path> {
  return path.parent().filter(|parent| return !parent.as_os_str().is_empty());
}

/// ログファイルを準備できなかったときのエラー型。
///
/// 開けなかった時点でビルドを止める — ログが残らないまま処理が進むより、指定が効いていないことを
/// 即座に知らせるほうがよい。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum LogFileError {
  /// ログファイルの親ディレクトリの作成エラー
  #[error("ログファイルの出力先ディレクトリを作成できませんでした: {path}")]
  #[diagnostic(
    code(cli::create_log_dir),
    help("--log-file のパスと、その親ディレクトリの書き込み権限を確認してください。")
  )]
  CreateLogDir {
    /// 親ディレクトリのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// ログファイルのオープンエラー
  #[error("ログファイルを開けませんでした: {path}")]
  #[diagnostic(
    code(cli::open_log_file),
    help("--log-file にはディレクトリではなく書き込み可能なファイルパスを指定してください。")
  )]
  Open {
    /// ログファイルのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

#[cfg(test)]
mod tests {
  use std::{fs, io::Write, path::Path};

  use super::{LogFileError, open_log_file, parent_to_create};

  #[test]
  fn creates_missing_parent_directories() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let path = dir.path().join("nested").join("deeper").join("build.log");

    let mut file = open_log_file(&path).expect("親ディレクトリを作ってから開けるはず");
    file.write_all(b"x").expect("書き込めるはず");

    assert!(path.exists(), "指定したパスにログファイルができる");
  }

  #[test]
  fn truncates_existing_file() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let path = dir.path().join("build.log");
    fs::write(&path, "前回の実行の記録").expect("事前の内容を書けるはず");

    drop(open_log_file(&path).expect("既存ファイルを開けるはず"));

    assert_eq!(fs::read_to_string(&path).expect("読めるはず"), "", "実行ごとに truncate する");
  }

  #[test]
  fn bare_file_name_has_no_directory_to_create() {
    assert_eq!(parent_to_create(Path::new("run.log")), None, "カレント直下なら作るディレクトリは無い");
    assert_eq!(parent_to_create(Path::new("logs/run.log")), Some(Path::new("logs")), "親があれば作る");
  }

  #[test]
  fn reports_unopenable_path() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

    let error = open_log_file(dir.path()).expect_err("ディレクトリは開けないはず");

    assert!(matches!(error, LogFileError::Open { .. }), "オープン失敗として報告する");
  }
}
