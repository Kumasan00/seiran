//! 失敗した `build` でも確定済みの警告が端末とログファイルへ出ることを、binary を起動して確かめる（#550）
//!
//! 警告の保持そのものは `seiran-compiler` の `compile_facade` が覆う。ここでは CLI の報告順序（確定済み警告 →
//! 主エラー）と、compile 成功後に保存が失敗した実行でも警告が消えないことを見る。render の失敗は注入できない
//! ので、保存の失敗（出力ディレクトリの親が通常ファイル）で「compile 成功後の失敗」を代表させる。
//! プロジェクトの組み立ては `tests/common` が担う。

mod common;

use std::{fs, path::Path};

use common::{seiran, stderr_text, write_project};

/// 拡張子が `.sei` でないソースに付く警告の code。
const WARNING_CODE: &str = "project::config::source_extension";

/// 出力ディレクトリを作れないときの診断 code。
const WRITE_ERROR_CODE: &str = "cli::create_output_dir";

/// 出力ディレクトリの親に通常ファイルを置き、PDF の保存を必ず失敗させる出力先を返す。
fn unwritable_output_dir(dir: &Path) -> &'static str {
  fs::write(dir.join("blocker"), "").expect("通常ファイルを書けるはず");
  return "blocker/out";
}

/// `text` の中に `before` と `after` が両方含まれ、`before` が `after` より前に現れることを確かめる。
fn assert_appears_before(text: &str, before: &str, after: &str) {
  assert!(text.contains(before), "{before} が出るはず: {text}");
  assert!(text.contains(after), "{after} が出るはず: {text}");
  let before_at = text.find(before).expect("直前の assert で含まれることを確かめた");
  let after_at = text.find(after).expect("直前の assert で含まれることを確かめた");
  assert!(before_at < after_at, "{before} は {after} より前に出るはず: {text}");
}

#[test]
fn warnings_survive_a_failed_write_on_the_terminal_and_in_the_log() {
  // Arrange — issue #550 の再現手順: `.txt` 入力（拡張子警告 1 件）で PDF の保存先を作れなくする
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let output_dir = unwritable_output_dir(dir.path());
  write_project(dir.path(), "doc.txt", "Hello, Seiran!", output_dir);

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"], None);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "保存の失敗は処理失敗: {stderr}");
  assert_appears_before(&stderr, WARNING_CODE, WRITE_ERROR_CODE);
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert_appears_before(&log, WARNING_CODE, WRITE_ERROR_CODE);
}

#[test]
fn quiet_keeps_warnings_of_a_failed_write_only_in_the_log() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let output_dir = unwritable_output_dir(dir.path());
  write_project(dir.path(), "doc.txt", "Hello, Seiran!", output_dir);

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-q", "--log-file", "x.log"], None);

  // Assert — `-q` が黙らせるのは端末の警告だけで、主エラーは端末にも出る
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1));
  assert!(!stderr.contains(WARNING_CODE), "-q では端末に警告を出さない: {stderr}");
  assert!(stderr.contains(WRITE_ERROR_CODE), "主エラーは端末に出る: {stderr}");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert_appears_before(&log, WARNING_CODE, WRITE_ERROR_CODE);
}

#[test]
fn warnings_of_a_failed_compile_come_before_the_error() {
  // Arrange — issue #550 の再現手順: `.txt` 入力に未知コマンドを足す
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.txt", "\\unknowncommand{x}", "out");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"], None);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "未知コマンドは処理失敗: {stderr}");
  assert_appears_before(&stderr, WARNING_CODE, "frontend::");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert_appears_before(&log, WARNING_CODE, "frontend::");
}

#[test]
fn successful_build_reports_warnings_before_the_summary() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.txt", "Hello, Seiran!", "out");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  // Assert — 成功した実行の順序（警告 → 成功サマリ）は #550 の前と同じ
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  assert_appears_before(&stderr, WARNING_CODE, "\u{2713}");
  assert!(dir.path().join("out/doc.pdf").is_file(), "PDF が保存されるはず");
}
