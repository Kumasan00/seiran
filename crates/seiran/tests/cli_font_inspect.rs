//! フォント調査サブコマンド（`variation-axes` / `ttc-names` / `script-langs`）の終了コードと出力先を、binary を
//! 起動して確かめる（#549）
//!
//! 一覧の書き出しの分類（`BrokenPipe` は成功・それ以外は失敗）は `subcommand::listing` の in-src テストが
//! 失敗する writer の注入で決定的に覆う。ここではプロセスとしての終了コード・stderr の診断・panic しないことを
//! 見る。フォントは `vendor/fonts/`（`tools/fetch-test-assets.sh` で取得）を使う。
//!
//! 失敗させるテストは一時ディレクトリをカレントにして短い相対パスを渡す — miette は 80 桁で折り返すので、
//! 長い絶対パスだとパスや OS エラー文が行をまたいで `contains` で見つからなくなる。

use std::{
  fs,
  path::{Path, PathBuf},
  process::{Command, Output, Stdio},
};

/// `vendor/fonts/` 内のフォントの絶対パスを返す。
fn vendor_font(name: &str) -> PathBuf {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts").join(name);
  assert!(
    path.is_file(),
    "{} を読めるはず（tools/fetch-test-assets.sh の実行が必要な場合があります）",
    path.display()
  );
  return path;
}

/// パスをコマンドライン引数の文字列にする。
fn path_arg(path: &Path) -> &str { return path.to_str().expect("テストで使うパスは UTF-8 のはず"); }

/// テスト対象の binary を `dir` をカレントディレクトリにして起動し、終了まで待つ。
///
/// 開発者の shell に `RUST_LOG` があると stderr が揺れるので外す。
fn seiran(dir: &Path, args: &[&str]) -> Output {
  return Command::new(env!("CARGO_BIN_EXE_seiran"))
    .args(args)
    .current_dir(dir)
    .env_remove("RUST_LOG")
    .output()
    .expect("seiran を起動できるはず");
}

/// 標準出力の読み手を、何も読まないうちに閉じて起動する（`| head -n0` 相当）。
///
/// 読み手を閉じてから子が最初の書き込みに到達するのが通常の順序で、そのとき子の書き込みは `BrokenPipe`
/// になる。子が先に書き切る順序になった実行ではこのテストは空振りする（成功条件は同じなので不安定には
/// ならない）— `BrokenPipe` の分類そのものは `subcommand::listing` の in-src テストが決定的に覆う。
fn seiran_with_closed_stdout(args: &[&str]) -> Output {
  let mut child = Command::new(env!("CARGO_BIN_EXE_seiran"))
    .args(args)
    .env_remove("RUST_LOG")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("seiran を起動できるはず");
  drop(child.stdout.take());
  return child.wait_with_output().expect("seiran の終了を待てるはず");
}

/// `Output` の stderr を文字列にする。
fn stderr_text(output: &Output) -> String { return String::from_utf8_lossy(&output.stderr).into_owned(); }

/// `Output` の stdout を文字列にする。
fn stdout_text(output: &Output) -> String { return String::from_utf8_lossy(&output.stdout).into_owned(); }

#[test]
fn ttc_names_survives_a_closed_reader() {
  let font = vendor_font("SourceHanCodeJP.ttc");

  let output = seiran_with_closed_stdout(&["ttc-names", path_arg(&font)]);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "受け手の終了は正常終了: {stderr}");
  assert!(!stderr.contains("panicked"), "panic しない: {stderr}");
}

#[test]
fn ttc_names_reports_a_missing_file_with_its_path() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

  let output = seiran(dir.path(), &["ttc-names", "missing.ttc"]);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "読めないファイルは処理失敗: {stderr}");
  assert!(stderr.contains("cli::ttc_names::read_file"), "読み込み失敗の診断: {stderr}");
  assert!(stderr.contains("missing.ttc"), "対象パスが出る: {stderr}");
  assert_eq!(stderr.matches("os error 2").count(), 1, "OS エラー文は cause に 1 回だけ: {stderr}");
}

#[test]
fn ttc_names_rejects_a_file_that_is_not_a_font() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  fs::write(dir.path().join("notes.txt"), "not a font").expect("フォントでないファイルを書けるはず");

  // Act
  let output = seiran(dir.path(), &["ttc-names", "notes.txt"]);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "フォントでないファイルは成功扱いにしない: {stderr}");
  assert!(stderr.contains("cli::ttc_names::file_parse"), "解析失敗の診断: {stderr}");
  assert!(stderr.contains("notes.txt"), "対象パスが出る: {stderr}");
  assert!(stdout_text(&output).is_empty(), "一覧は 1 行も出さない");
}

#[test]
fn missing_argument_is_a_usage_error() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

  let output = seiran(dir.path(), &["ttc-names"]);

  assert_eq!(output.status.code(), Some(2), "引数エラーは clap の終了コード 2");
}

/// `/dev/full` は Linux にしかない（CI は ubuntu で走る）。
#[cfg(target_os = "linux")]
#[test]
fn full_stdout_is_a_failure() {
  // Arrange
  let font = vendor_font("SourceHanCodeJP.ttc");
  let dev_full = fs::OpenOptions::new().write(true).open("/dev/full").expect("/dev/full を開けるはず");

  // Act
  let output = Command::new(env!("CARGO_BIN_EXE_seiran"))
    .args(["ttc-names", path_arg(&font)])
    .env_remove("RUST_LOG")
    .stdout(dev_full)
    .output()
    .expect("seiran を起動できるはず");

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "受け手の終了以外の書き込み失敗は処理失敗: {stderr}");
  assert!(stderr.contains("cli::write_stdout"), "書き込み失敗の診断: {stderr}");
}
