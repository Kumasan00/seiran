//! 失敗した `build` でも確定済みの警告が端末とログファイルへ出ることを、binary を起動して確かめる（#550）
//!
//! 警告の保持そのものは `seiran-compiler` の `compile_facade` が覆う。ここでは CLI の報告順序（確定済み警告 →
//! 主エラー）と、compile 成功後に保存が失敗した実行でも警告が消えないことを見る。render の失敗は注入できない
//! ので、保存の失敗（出力ディレクトリの親が通常ファイル）で「compile 成功後の失敗」を代表させる。
//! フォントは `vendor/fonts/STIXTwoMath-Regular.ttf`（`tools/fetch-test-assets.sh` で取得）を 19 種別すべてに使う。

use std::{
  fs,
  path::{Path, PathBuf},
  process::{Command, Output},
};

use seiran_compiler::test_support;

/// 拡張子が `.sei` でないソースに付く警告の code。
const WARNING_CODE: &str = "project::config::source_extension";

/// 出力ディレクトリを作れないときの診断 code。
const WRITE_ERROR_CODE: &str = "cli::create_output_dir";

/// テストフォントの絶対パスを返す。
fn test_font() -> PathBuf {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
  assert!(
    path.is_file(),
    "{} を読めるはず（tools/fetch-test-assets.sh の実行が必要な場合があります）",
    path.display()
  );
  return path;
}

/// `doc.txt` を唯一のソースにし、PDF を `output_dir`（`dir` からの相対）へ出す config.toml と本文を `dir` へ書く。
fn write_project(dir: &Path, body: &str, output_dir: &str) {
  let font = test_font();
  let config = format!(
    "sources = [\"doc.txt\"]\n\n{}{}{}",
    test_support::valid_output_section("doc", output_dir),
    test_support::valid_pdf_section(),
    test_support::make_font_sections(font.to_str().expect("テストフォントのパスは UTF-8 のはず")),
  );
  fs::write(dir.join("config.toml"), config).expect("config.toml を書けるはず");
  fs::write(dir.join("doc.txt"), body).expect("doc.txt を書けるはず");
}

/// 出力ディレクトリの親に通常ファイルを置き、PDF の保存を必ず失敗させる出力先を返す。
fn unwritable_output_dir(dir: &Path) -> &'static str {
  fs::write(dir.join("blocker"), "").expect("通常ファイルを書けるはず");
  return "blocker/out";
}

/// テスト対象の binary を `dir` をカレントディレクトリにして起動する。
///
/// 開発者の shell に `RUST_LOG` があると stderr とファイルの内容が揺れるので外す。
fn seiran(dir: &Path, args: &[&str]) -> Output {
  return Command::new(env!("CARGO_BIN_EXE_seiran"))
    .args(args)
    .current_dir(dir)
    .env_remove("RUST_LOG")
    .output()
    .expect("seiran を起動できるはず");
}

/// `Output` の stderr を文字列にする。
fn stderr_text(output: &Output) -> String { return String::from_utf8_lossy(&output.stderr).into_owned(); }

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
  write_project(dir.path(), "Hello, Seiran!", output_dir);

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"]);

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
  write_project(dir.path(), "Hello, Seiran!", output_dir);

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-q", "--log-file", "x.log"]);

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
  write_project(dir.path(), "\\unknowncommand{x}", "out");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"]);

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
  write_project(dir.path(), "Hello, Seiran!", "out");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml"]);

  // Assert — 成功した実行の順序（警告 → 成功サマリ）は #550 の前と同じ
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  assert_appears_before(&stderr, WARNING_CODE, "\u{2713}");
  assert!(dir.path().join("out/doc.pdf").is_file(), "PDF が保存されるはず");
}
