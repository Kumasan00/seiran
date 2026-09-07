//! `--log-file` の書き出し先（ログファイルを開く処理と、そこへ書くための sink）
//!
//! ログファイルは「1 回の実行の記録」なので、実行のたびに新規作成する（既存パスは拒否して入力を守る）。
//! tracing の layer とユーザー向け報告（warning 診断・成功サマリ・致命的エラー診断）は同じ [`LogSink`] を
//! 共有し、1 本の同期 writer 越しに書く。書き込み・flush の失敗は捨てず最初の 1 件を保持して
//! [`LogSink::finish`] で取り出す — tracing の writer が返したエラーは呼び出し元の `Result` へ伝わらないので、
//! sink 自身が保持しないと「記録できていない実行」を成功として終えてしまう。
//!
//! `finish` を呼ばずに落ちた実行（`run` 内の panic 等）でも [`LogSink`] の `Drop` が書き残しを流し切る。
//! ただしその経路では保持した失敗を報告する主体がいない（`finish` を経由しないので `LogFailure` を
//! 受け取る側が存在しない）。

use std::{
  fs::{self, File},
  io::{self, BufWriter, Write},
  path::{Path, PathBuf},
  sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use miette::Diagnostic;
use thiserror::Error;
use tracing_subscriber::fmt::MakeWriter;

/// ログファイルへの書き出し口。
///
/// tracing の layer へ渡す writer と、warning 診断・成功サマリ・致命的エラー診断を直接書く経路の両方が
/// 同じ状態を共有する。経路を 1 本に保つのは、イベントと報告の前後関係を崩さないためと、失敗の保持と
/// flush の完了を 1 箇所（[`LogSink::finish`]）へ集約するため。
pub(super) struct LogSink {
  /// 書き出し先と保持した失敗（layer 側と共有する）
  state: Arc<Mutex<SinkState>>,
  /// 失敗の報告に使うログファイルのパス
  path: PathBuf,
}

/// 書き出し先と、その実行で最初に起きた I/O 失敗。
struct SinkState {
  /// ログファイルへの同期 writer
  writer: BufWriter<Box<dyn Write + Send>>,
  /// 最初の書き込み・flush 失敗（後続の失敗で上書きしない）
  first_error: Option<io::Error>,
}

impl SinkState {
  /// I/O の結果を検査し、最初の失敗だけを保持したうえで結果をそのまま返す。
  ///
  /// 呼び出し側（tracing の layer）はエラーを握り潰すが、保持したぶんが `finish` で報告される。
  fn check<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
    return match result {
      Ok(value) => Ok(value),
      Err(error) => {
        let kind = error.kind();
        let message = error.to_string();
        if self.first_error.is_none() {
          self.first_error = Some(error);
        }
        Err(io::Error::new(kind, message))
      },
    };
  }
}

/// 毒された `Mutex` でも記録を続けるためのロック。
///
/// ログの writer は「壊れたら以後書けない」種類の状態ではなく、panic 中の実行でも失敗の保持だけは
/// 続けたいので、毒は無視して中身を取り出す。
fn lock(state: &Mutex<SinkState>) -> MutexGuard<'_, SinkState> {
  return state.lock().unwrap_or_else(PoisonError::into_inner);
}

/// tracing の layer へ渡す writer 生成器。
///
/// [`LogSink`] と同じ状態を共有するので、layer 側の書き込み失敗も `finish` から取り出せる。
#[derive(Clone)]
pub(super) struct LogWriter {
  /// [`LogSink`] と共有する書き出し先
  state: Arc<Mutex<SinkState>>,
}

impl<'writer> MakeWriter<'writer> for LogWriter {
  type Writer = SinkGuard<'writer>;

  fn make_writer(&'writer self) -> Self::Writer { return SinkGuard(lock(&self.state)); }
}

/// イベント 1 件を書いている間だけ writer を占有するガード。
///
/// event の複数回の `write` が診断ブロックの行と混ざらないよう、1 件のあいだロックを保持する。
pub(super) struct SinkGuard<'sink>(MutexGuard<'sink, SinkState>);

impl Write for SinkGuard<'_> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let result = self.0.writer.write(buf);
    return self.0.check(result);
  }

  fn flush(&mut self) -> io::Result<()> {
    let result = self.0.writer.flush();
    return self.0.check(result);
  }
}

/// ログの記録に失敗したという事実（診断の体裁は報告側が決める）。
///
/// 同じ失敗でも「本処理は完了した実行」と「本処理も失敗した実行」で書くべき説明が違うので、
/// sink は生の事実だけを返し、文言は `termination` が付ける。
///
/// 可視性が `pub(crate)` なのは、文言を付ける `termination` module が `reporting` の外にあるため
/// （`LogFileError` と同じ扱い）。
#[derive(Debug)]
pub(crate) struct LogFailure {
  /// ログファイルのパス
  pub(crate) path: String,
  /// 最初に起きた I/O 失敗
  pub(crate) source: io::Error,
}

impl LogSink {
  /// `path` を新規作成して開き、書き出し口を作る。
  ///
  /// # Errors
  ///
  /// 親ディレクトリを作れない、パスが既に存在する、またはファイルを開けないときに [`LogFileError`] を返す。
  pub(super) fn open(path: &Path) -> Result<Self, LogFileError> {
    let file = open_log_file(path)?;
    return Ok(LogSink::from_writer(path.to_path_buf(), Box::new(file)));
  }

  /// 任意の書き出し先から作る。
  ///
  /// ファイル以外を渡せる入口を持つのは、書き込み失敗の注入をテストから行うため（実ファイルの
  /// 書き込み失敗は移植可能な形で起こせない）。
  fn from_writer(path: PathBuf, writer: Box<dyn Write + Send>) -> Self {
    return LogSink {
      state: Arc::new(Mutex::new(SinkState {
        writer: BufWriter::new(writer),
        first_error: None,
      })),
      path,
    };
  }

  /// tracing の layer へ渡す writer 生成器を作る。
  pub(super) fn writer(&self) -> LogWriter {
    return LogWriter {
      state: Arc::clone(&self.state),
    };
  }

  /// ユーザー向け報告 1 件ぶんをファイルへ書く（末尾に改行を足す）。
  ///
  /// 失敗はその場では報告しない — 報告の途中で処理を分岐させず、[`LogSink::finish`] が 1 度だけ返す。
  pub(super) fn write_block(&self, text: &str) {
    let mut guard = lock(&self.state);
    let written = writeln!(guard.writer, "{text}");
    let _ = guard.check(written);
  }

  /// 書き残しを流し切り、保持していた最初の失敗を返す。
  ///
  /// 保証するのは OS への書き込みと flush の完了までで、電源断まで含めた永続化は保証しない。
  ///
  /// # Errors
  ///
  /// 書き込みまたは flush が失敗していたとき [`LogFailure`] を返す。
  pub(super) fn finish(self) -> Result<(), LogFailure> {
    let mut guard = lock(&self.state);
    let flushed = guard.writer.flush();
    let _ = guard.check(flushed);
    let first_error = guard.first_error.take();
    // `self` の `Drop`（同じ Mutex を取り直して flush する）より前に手放す — 暗黙の drop 順に頼らない。
    drop(guard);
    return match first_error {
      Some(source) => Err(LogFailure {
        path: self.path.display().to_string(),
        source,
      }),
      None => Ok(()),
    };
  }
}

impl Drop for LogSink {
  /// `finish` を呼ばずに落ちた実行でも書き残しを流し切る。
  ///
  /// tracing の layer（[`LogWriter`]）が同じ状態をもう 1 つ `Arc` で保持しているため、この drop で
  /// `SinkState` そのものは解放されない。それでも `flush` は OS への書き込みを進めるので、`finish` を
  /// 経由しない panic 中の unwind でも `BufWriter` に溜まった内容は失われない。ここで捕まえた失敗は
  /// 読む主体がいない（`finish` を経ない経路なので `LogFailure` を受け取る側が存在しない）ため捨てる。
  fn drop(&mut self) {
    let mut guard = lock(&self.state);
    let flushed = guard.writer.flush();
    let _ = guard.check(flushed);
  }
}

/// ログファイルを新規作成して開く。
///
/// 既存パスは truncate せずエラーにする — `--log-file` に入力ファイルを渡した実行でその入力を壊さないため。
/// 親ディレクトリが無ければ作る — 出力先を掘ってから実行し直す手間を、ログの指定ごときで掛けさせないため。
fn open_log_file(path: &Path) -> Result<File, LogFileError> {
  if let Some(parent) = parent_to_create(path) {
    fs::create_dir_all(parent).map_err(|source| {
      return LogFileError::CreateLogDir {
        path: parent.display().to_string(),
        source,
      };
    })?;
  }
  return File::create_new(path).map_err(|source| {
    // 既存パスは truncate せず拒否する — ログの指定で入力（設定・本文・フォント・画像・CSL）を壊さないため。
    // `O_CREAT|O_EXCL` なので、判定と作成の間に割り込まれる余地が無い。
    if source.kind() == io::ErrorKind::AlreadyExists {
      return LogFileError::AlreadyExists {
        path: path.display().to_string(),
      };
    }
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
    source: io::Error,
  },

  /// 既に存在するパスへのログ出力の拒否
  #[error("ログファイルの出力先が既に存在します: {path}")]
  #[diagnostic(
    code(cli::log_file_exists),
    help("--log-file には毎回新しいパスを指定してください（既存のファイルは上書きしません）。")
  )]
  AlreadyExists {
    /// 指定されたログファイルのパス
    path: String,
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
    source: io::Error,
  },
}

#[cfg(test)]
mod tests {
  use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
  };

  use super::{LogFileError, LogSink, open_log_file, parent_to_create};

  /// 書き込みも flush も必ず失敗する書き出し先。
  ///
  /// 呼ばれるたびに違うメッセージを返し、「保持されるのは最初の 1 件」を見分けられるようにする。
  struct FailingWriter {
    /// 何回目の呼び出しか
    calls: usize,
  }

  impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
      self.calls += 1;
      return Err(io::Error::other(format!("{} 回目の書き込み失敗", self.calls)));
    }

    fn flush(&mut self) -> io::Result<()> {
      self.calls += 1;
      return Err(io::Error::other(format!("{} 回目の flush 失敗", self.calls)));
    }
  }

  /// 書かれた内容を後から検査できる書き出し先。
  #[derive(Clone)]
  struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

  impl Write for SharedBuffer {
    #[expect(
      clippy::unwrap_in_result,
      reason = "テスト専用の Mutex で他スレッドから触られることはなく毒されないため、panic し得ない"
    )]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      self.0.lock().expect("テスト内でロックが毒されることはない").extend_from_slice(buf);
      return Ok(buf.len());
    }

    fn flush(&mut self) -> io::Result<()> { return Ok(()) }
  }

  #[test]
  fn successful_writes_reach_the_writer_and_finish_cleanly() {
    // Arrange
    let buffer = SharedBuffer(Arc::new(Mutex::new(Vec::new())));
    let sink = LogSink::from_writer(PathBuf::from("run.log"), Box::new(buffer.clone()));

    // Act
    sink.write_block("記録する 1 行");
    sink.finish().expect("書き込みが成功した実行は失敗を持たない");

    // Assert
    let written = buffer.0.lock().expect("テスト内でロックが毒されることはない").clone();
    assert_eq!(String::from_utf8(written).expect("UTF-8 のはず"), "記録する 1 行\n", "末尾に改行を足して書く");
  }

  #[test]
  fn dropping_without_finish_still_flushes_buffered_content() {
    // Arrange — tracing の layer が `writer()` で `SinkState` をもう 1 つの `Arc` として握り続ける状況を再現する。
    // `sink` だけが所有者なら drop で参照カウントが 0 になり `BufWriter` 自身の drop-flush で届いてしまい、
    // `LogSink` の `Drop` を消しても通ってしまう（判別力が無い）。`writer` を生かしたまま `sink` を drop することで、
    // `SinkState` は生き残ったまま（参照カウント 1）flush が必要になる、実際の panic 経路と同じ状況を作る。
    let buffer = SharedBuffer(Arc::new(Mutex::new(Vec::new())));
    let sink = LogSink::from_writer(PathBuf::from("run.log"), Box::new(buffer.clone()));
    let writer = sink.writer();

    // Act — `finish` を呼ばず panic 中の unwind を模す。`writer` はまだ生きているので `SinkState` は解放されない。
    sink.write_block("記録する 1 行");
    drop(sink);

    // Assert
    let written = buffer.0.lock().expect("テスト内でロックが毒されることはない").clone();
    assert_eq!(
      String::from_utf8(written).expect("UTF-8 のはず"),
      "記録する 1 行\n",
      "finish を経由せず、他の Arc が生きたまま drop されても書き残しを流す"
    );
    // `writer` をここまで生かして「共有所有で SinkState が解放されない」状況を保証する。
    drop(writer);
  }

  #[test]
  fn finish_reports_the_write_failure() {
    let sink = LogSink::from_writer(PathBuf::from("run.log"), Box::new(FailingWriter { calls: 0 }));

    sink.write_block("記録できない 1 行");
    let failure = sink.finish().expect_err("書き込みに失敗した実行は失敗を報告する");

    assert_eq!(failure.path, "run.log", "失敗はログのパスとともに報告する");
  }

  #[test]
  fn only_the_first_failure_is_kept() {
    // Arrange — BufWriter の容量（8 KiB）を超える書き込みは素通しになるので、write_block ごとに失敗が起きる
    let sink = LogSink::from_writer(PathBuf::from("run.log"), Box::new(FailingWriter { calls: 0 }));
    let long_line = "あ".repeat(8 * 1024);

    // Act
    sink.write_block(&long_line);
    sink.write_block(&long_line);
    let failure = sink.finish().expect_err("失敗を報告する");

    // Assert
    assert!(failure.source.to_string().contains("1 回目"), "後続の失敗で上書きしない: {}", failure.source);
  }

  #[test]
  fn creates_missing_parent_directories() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let path = dir.path().join("nested").join("deeper").join("build.log");

    let mut file = open_log_file(&path).expect("親ディレクトリを作ってから開けるはず");
    file.write_all(b"x").expect("書き込めるはず");

    assert!(path.exists(), "指定したパスにログファイルができる");
  }

  #[test]
  fn refuses_existing_file_without_touching_it() {
    // Arrange
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let path = dir.path().join("build.log");
    fs::write(&path, "前回の実行の記録").expect("事前の内容を書けるはず");

    // Act
    let error = open_log_file(&path).expect_err("既存ファイルは拒否するはず");

    // Assert
    assert!(matches!(error, LogFileError::AlreadyExists { .. }), "既存パスとして報告する");
    assert_eq!(
      fs::read_to_string(&path).expect("読めるはず"),
      "前回の実行の記録",
      "拒否した実行はファイルへ触らない"
    );
  }

  #[test]
  fn bare_file_name_has_no_directory_to_create() {
    assert_eq!(parent_to_create(Path::new("run.log")), None, "カレント直下なら作るディレクトリは無い");
    assert_eq!(parent_to_create(Path::new("logs/run.log")), Some(Path::new("logs")), "親があれば作る");
  }

  #[test]
  fn refuses_an_existing_directory() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

    let error = open_log_file(dir.path()).expect_err("既存のディレクトリは拒否するはず");

    assert!(matches!(error, LogFileError::AlreadyExists { .. }), "存在するパスとして報告する");
  }
}
