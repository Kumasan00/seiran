//! 工程の記録・実行記録・`RUST_LOG` の通知が、`-q` / `-v` / `RUST_LOG` の組み合わせごとに端末とログファイルへ
//! どう出るかを、binary を起動して確かめる（#551）
//!
//! compiler 内の工程（`compile` とその子）の契約は `seiran-compiler` の `tests/trace_events.rs` が固定する。
//! ここでは CLI が開く `render` / `write` の工程と、出力先ごとの振り分け（`Reporter` の構造）を見る。

mod common;

use std::{fs, path::Path};

use common::{seiran, stderr_text, write_project};

/// 成功する本文（`.sei` なので拡張子の警告は出ない）。
const BODY: &str = "Hello, Seiran!";

/// 成功するプロジェクトを `dir` へ書く。
fn write_ok_project(dir: &Path) { write_project(dir, "doc.sei", BODY, "out"); }

#[test]
fn verbose_terminal_shows_start_and_end_of_render_and_write() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v"], None);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  for prefix in ["compile:", "compile:frontend:", "render:", "write:"] {
    assert!(
      stderr
        .lines()
        .any(|line| return line.contains(&format!("{prefix} ")) && line.contains("工程を開始")),
      "{prefix} の開始が出るはず: {stderr}"
    );
    assert!(
      stderr.lines().any(|line| {
        return line.contains(&format!("{prefix} "))
          && line.contains("工程を終了")
          && line.contains("status=Succeeded")
          && line.contains("elapsed=");
      }),
      "{prefix} の成功した終了が出るはず: {stderr}"
    );
  }
}

#[test]
fn failed_write_records_a_failed_end_in_the_log() {
  // Arrange — 出力ディレクトリの親を通常ファイルにして保存を必ず失敗させる
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  fs::write(dir.path().join("blocker"), "").expect("通常ファイルを書けるはず");
  write_project(dir.path(), "doc.sei", BODY, "blocker/out");

  // Act
  let output = seiran(
    dir.path(),
    &[
      "build",
      "-c",
      "config.toml",
      "-q",
      "-v",
      "--log-file",
      "x.log",
    ],
    None,
  );

  // Assert
  assert_eq!(output.status.code(), Some(1), "保存の失敗は処理失敗: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(
    log.lines().any(|line| return line.contains("render: ") && line.contains("status=Succeeded")),
    "描画は成功で終わる: {log}"
  );
  assert!(
    log.lines().any(|line| {
      return line.contains("write: ") && line.contains("工程を終了") && line.contains("status=Failed");
    }),
    "保存は失敗で終わる: {log}"
  );
}

#[test]
fn failed_frontend_is_recorded_without_duplicating_the_diagnostic() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.sei", "\\unknowncommand{x}", "out");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v", "--log-file", "x.log"], None);

  // Assert
  assert_eq!(output.status.code(), Some(1), "未知コマンドは処理失敗: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(
    log.lines().any(|line| return line.contains("compile:frontend: ") && line.contains("工程を開始")),
    "frontend の開始が残る: {log}"
  );
  assert!(
    log.lines().any(|line| {
      return line.contains("compile:frontend: ")
        && line.contains("工程を終了")
        && line.contains("status=Failed")
        && line.contains("elapsed=");
    }),
    "frontend の失敗した終了が残る: {log}"
  );
  assert!(!log.contains("compile:semantics: "), "失敗した工程の後段は始まらない: {log}");
  assert_eq!(
    log.matches("frontend::eval::unknown_command").count(),
    1,
    "診断は致命的エラーの記録 1 回だけで、tracing へは複製しない: {log}"
  );
}

/// 不正な `RUST_LOG` の通知に付く診断 code。
const INVALID_CODE: &str = "cli::rust_log::invalid";

/// `RUST_LOG` が `-v` を覆う通知に付く診断 code。
const OVERRIDE_CODE: &str = "cli::rust_log::overrides_verbose";

#[test]
fn override_notice_survives_a_rust_log_that_hides_warn() {
  // Arrange — `error` は WARN を通さないので、tracing の WARN だった通知は以前は消えていた
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  // Act
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v", "--log-file", "x.log"], Some("error"));

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  assert!(stderr.contains(OVERRIDE_CODE), "端末に通知が出る: {stderr}");
  assert!(!stderr.contains("工程を開始"), "実効フィルタは RUST_LOG=error のまま: {stderr}");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains(OVERRIDE_CODE), "ファイルにも通知が残る: {log}");
}

#[test]
fn override_notice_survives_a_target_limited_rust_log() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v"], Some("seiran_compiler=info"));

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  assert!(stderr.contains(OVERRIDE_CODE), "target 限定の RUST_LOG でも通知が出る: {stderr}");
  assert!(stderr.contains("compile:input: "), "RUST_LOG が通す compiler の工程は出る: {stderr}");
  assert!(!stderr.contains("render: "), "RUST_LOG が通さない seiran target の工程は出ない: {stderr}");
}

#[test]
fn quiet_hides_the_notice_only_from_the_terminal() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(
    dir.path(),
    &[
      "build",
      "-c",
      "config.toml",
      "-q",
      "-v",
      "--log-file",
      "x.log",
    ],
    Some("error"),
  );

  assert_eq!(output.status.code(), Some(0));
  assert!(output.stderr.is_empty(), "-q の端末は無言: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains(OVERRIDE_CODE), "-q でもファイルには通知が残る: {log}");
}

#[test]
fn invalid_rust_log_is_a_warning_diagnostic_on_both_sinks() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"], Some("seiran=not-a-level"));

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "通知だけで処理は成功する: {stderr}");
  assert!(stderr.contains(INVALID_CODE), "端末に通知が出る: {stderr}");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains(INVALID_CODE), "ファイルにも通知が残る: {log}");
}
