//! 工程の記録・実行記録・`RUST_LOG` の通知が、`-q` / `-v` / `RUST_LOG` の組み合わせごとに端末とログファイルへ
//! どう出るかを、binary を起動して確かめる
//!
//! compiler 内の工程（`compile` とその子）の契約は `seiran-compiler` の `tests/trace_events.rs` が固定する。
//! ここでは CLI が開く `render` / `write` の工程と、出力先ごとの振り分け（`Reporter` の構造）を見る。
//! 端末（stderr）へ書けない実行でも、終了コードとログファイルへの記録が変わらないことも見る。

mod common;

use std::{
  fs, io,
  path::Path,
  process::{Command, ExitStatus},
};

use common::{seiran, stderr_text, write_project};

/// 成功する本文（`.sei` なので拡張子の警告は出ない）。
const BODY: &str = "Hello, Seiran!";

/// 成功するプロジェクトを `dir` へ書く。
fn write_ok_project(dir: &Path) { write_project(dir, "doc.sei", BODY, "out"); }

#[test]
fn verbose_terminal_shows_one_activity_per_phase_and_file_keeps_the_results() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());
  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v", "--log-file", "run.log"], None);
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "{stderr}");
  let mut lines = stderr.lines();
  for activity in [
    "文書をコンパイル中",
    "設定と入力ファイルを読み込み中",
    "ソースを解析中",
    "参照と引用を解決中",
    "組版用フォントを準備中",
    "文書を組版中",
    "PDF を描画中",
    "PDF を保存中",
  ] {
    let line = lines.next().expect("工程の開始が 1 行ある");
    assert_eq!(line, activity, "{stderr}");
  }
  assert!(lines.next().is_some_and(|line| return line.contains("out/doc.pdf")), "{stderr}");
  assert!(lines.next().is_none(), "{stderr}");
  let log = fs::read_to_string(dir.path().join("run.log")).expect("ログを読めるはず");
  assert_eq!(log.matches("status=Succeeded").count(), 8, "{log}");
  assert!(log.contains("compile:frontend: ソースを解析中"), "{log}");
}

#[test]
fn font_inspections_record_the_phase_even_when_the_input_cannot_be_opened() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  for (cmd, activity) in [
    ("ttc-names", "フォント名を調査中"),
    ("variation-axes", "バリエーション軸を調査中"),
    ("script-langs", "スクリプトと言語を調査中"),
  ] {
    let path = format!("{cmd}.log");
    let output = seiran(dir.path(), &[cmd, "missing.ttf", "-v", "--log-file", &path], None);
    let stderr = stderr_text(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(output.stdout.is_empty(), "フォントを調査できなかったときは一覧を出さない");
    assert!(stderr.lines().next().is_some_and(|line| return line.ends_with(activity)), "{stderr}");
    assert!(!stderr.contains("status="), "{stderr}");
    let log = fs::read_to_string(dir.path().join(path)).expect("ログを読めるはず");
    let end = log.lines().find(|line| return line.contains("status=Failed")).expect("失敗した終了行がある");
    assert!(end.contains("elapsed=") && end.contains("font_path=missing.ttf"), "{end}");
    assert!(log.contains("cli::font_file::read_file"), "{log}");
  }
}

#[test]
fn failed_write_records_a_failed_end_in_the_log() {
  // 出力ディレクトリの親を通常ファイルにして保存を必ず失敗させる
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  fs::write(dir.path().join("blocker"), "").expect("通常ファイルを書けるはず");
  write_project(dir.path(), "doc.sei", BODY, "blocker/out");

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
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.sei", "\\unknowncommand{x}", "out");

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v", "--log-file", "x.log"], None);

  assert_eq!(output.status.code(), Some(1), "未知コマンドは処理失敗: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(
    log
      .lines()
      .any(|line| return line.contains("compile:frontend: ") && line.contains("ソースを解析中")),
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

#[test]
fn phase_targets_keep_names_when_filtered_on_their_own() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());
  let output =
    seiran(dir.path(), &["build", "-c", "config.toml"], Some("seiran_compiler::phase=info,seiran::phase=info"));
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "{stderr}");
  for prefix in ["compile:input:", "compile:typeset:", "render:", "write:"] {
    assert!(
      stderr.lines().any(|line| return line.contains(&format!("{prefix} ")) && line.ends_with('中')),
      "{stderr}"
    );
    assert!(stderr.contains(&format!("{prefix} 工程を終了 status=Succeeded")), "{stderr}");
  }
}

/// 不正な `RUST_LOG` の通知に付く診断 code。
const INVALID_CODE: &str = "cli::rust_log::invalid";

/// `RUST_LOG` が `-v` を覆う通知に付く診断 code。
const OVERRIDE_CODE: &str = "cli::rust_log::overrides_verbose";

#[test]
fn override_notice_survives_a_rust_log_that_hides_warn() {
  // `error` は WARN を通さないので、通知を tracing の WARN で出すと消える
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "-v", "--log-file", "x.log"], Some("error"));

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  assert!(stderr.contains(OVERRIDE_CODE), "端末に通知が出る: {stderr}");
  assert!(!stderr.contains("文書をコンパイル中"), "実効フィルタは RUST_LOG=error のまま: {stderr}");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains(OVERRIDE_CODE), "ファイルにも通知が残る: {log}");
  assert!(!log.contains("文書をコンパイル中"), "ファイルでも RUST_LOG=error を優先する: {log}");
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
fn invalid_rust_log_is_a_warning_diagnostic_on_both_sinks() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"], Some("seiran=not-a-level"));

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "通知だけで処理は成功する: {stderr}");
  assert!(stderr.contains(INVALID_CODE), "端末に通知が出る: {stderr}");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains(INVALID_CODE), "ファイルにも通知が残る: {log}");
  assert!(
    log.contains("compile:input: 設定と入力ファイルを読み込み中"),
    "ファイルは既定の工程記録へ戻る: {log}"
  );
}

/// ログファイルの空行でない行を集める。
fn non_empty_lines(log: &str) -> Vec<&str> {
  return log.lines().filter(|line| return !line.trim().is_empty()).collect();
}

#[test]
fn log_file_alone_records_progress_without_printing_it_to_stderr() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"], None);

  assert_eq!(output.status.code(), Some(0), "成功するはず: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  let lines = non_empty_lines(&log);
  assert_eq!(lines.first(), Some(&"# seiran 実行記録"), "先頭は実行記録: {log}");
  for key in [
    "開始時刻: ",
    "バージョン: ",
    r#"サブコマンド: build -c "config.toml""#,
    "基準ディレクトリ: ",
    "実効フィルタ: ",
  ] {
    assert!(lines.iter().take(6).any(|line| return line.starts_with(key)), "ヘッダに {key:?} がある: {log}");
  }
  let canonical_dir = dir.path().canonicalize().expect("一時ディレクトリを canonicalize できるはず");
  assert!(
    log.contains(&canonical_dir.display().to_string()) || log.contains(&dir.path().display().to_string()),
    "基準ディレクトリは起動時のカレントディレクトリ: {log}"
  );
  assert!(!stderr_text(&output).contains("文書をコンパイル中"), "端末に工程が出ない");
  assert_eq!(log.lines().filter(|line| return line.ends_with('中')).count(), 8, "全工程の開始を記録する: {log}");
  assert_eq!(log.matches("status=Succeeded").count(), 8, "全工程の結果を記録する: {log}");
  assert!(log.contains('\u{2713}'), "成功サマリは残る: {log}");
  let footer = &lines[lines.len() - 3..];
  assert_eq!(footer[0], "# seiran 実行終了", "末尾は終了記録: {log}");
  assert!(footer[1].starts_with("終了時刻: "), "終了記録に終了時刻がある: {log}");
  assert_eq!(footer[2], "終了状態: 成功", "成功した実行の終了状態: {log}");
}

#[test]
fn failed_run_ends_with_a_failed_status_after_the_diagnostic() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

  let output = seiran(dir.path(), &["build", "-c", "missing.toml", "-q", "--log-file", "x.log"], None);

  assert_eq!(output.status.code(), Some(1));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  let lines = non_empty_lines(&log);
  assert_eq!(lines.last(), Some(&"終了状態: 失敗"), "末尾は失敗の終了記録: {log}");
  let diagnostic_at = log.find("project::config::read_file").expect("致命的エラーの診断が残る");
  let footer_at = log.find("# seiran 実行終了").expect("終了記録が残る");
  assert!(diagnostic_at < footer_at, "終了記録は診断の後: {log}");
}

#[test]
fn header_records_the_effective_filter_and_precedes_the_notice() {
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
  assert!(output.stderr.is_empty(), "-q の端末には通知も出ない: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains("実効フィルタ: error"), "実効フィルタは RUST_LOG の値: {log}");
  let header_at = log.find("# seiran 実行記録").expect("実行記録が残る");
  let notice_at = log.find(OVERRIDE_CODE).expect("通知が残る");
  assert!(header_at < notice_at, "実行記録が通知より前: {log}");
}

#[test]
fn verbose_log_file_records_every_phase_between_header_and_footer() {
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
    None,
  );

  assert_eq!(output.status.code(), Some(0));
  assert!(output.stderr.is_empty(), "-q の端末は無言: {}", stderr_text(&output));
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  let filter = log.lines().find_map(|line| return line.strip_prefix("実効フィルタ: ")).expect("実効フィルタが残る");
  for directive in [
    "warn",
    "seiran=info",
    "seiran_compiler=info",
    "seiran_pdf=info",
  ] {
    assert!(filter.split(',').any(|part| return part == directive), "{filter:?} に {directive} がある");
  }
  let footer_at = log.find("# seiran 実行終了").expect("終了記録が残る");
  for prefix in [
    "compile:",
    "compile:input:",
    "compile:frontend:",
    "compile:semantics:",
    "compile:font:",
    "compile:typeset:",
    "render:",
    "write:",
  ] {
    let end_line = log
      .lines()
      .find(|line| return line.contains(&format!("{prefix} ")) && line.contains("工程を終了"))
      .unwrap_or_else(|| panic!("{prefix} の終了が残る: {log}"));
    assert!(end_line.contains("status=Succeeded"), "{end_line:?}");
    assert!(log.find(end_line).expect("直前で見つけた行") < footer_at, "工程は終了記録より前: {log}");
  }
}

#[test]
fn terminal_without_flags_shows_only_the_summary() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "成功するはず: {stderr}");
  assert!(stderr.contains('\u{2713}'), "成功サマリが出る: {stderr}");
  assert!(stderr.contains("out/doc.pdf · 1 ページ · "), "サマリは保存先・ページ数・所要時間を出す: {stderr}");
  assert!(!stderr.contains("文書をコンパイル中"), "既定では工程の event は出ない: {stderr}");
  assert!(!stderr.contains("# seiran 実行記録"), "実行記録はファイルだけ: {stderr}");
}

/// 拡張子が `.sei` でないソースに付く警告の code。
const SOURCE_EXTENSION_CODE: &str = "project::config::source_extension";

/// 読み手を閉じたパイプを stderr にして binary を `dir` をカレントディレクトリにして起動し、終了状態を返す。
///
/// 読み手は起動前に閉じるので、子の stderr への書き込みはすべて `BrokenPipe` で失敗する（Rust の実行時は
/// SIGPIPE を無視するので、シグナルで死なずに書き込みエラーになる）。
fn seiran_with_closed_stderr(dir: &Path, args: &[&str]) -> ExitStatus {
  let (reader, writer) = io::pipe().expect("パイプを作れるはず");
  drop(reader);
  return Command::new(env!("CARGO_BIN_EXE_seiran"))
    .args(args)
    .current_dir(dir)
    .env_remove("RUST_LOG")
    .stderr(writer)
    .status()
    .expect("seiran を起動できるはず");
}

#[test]
fn closed_terminal_still_saves_the_pdf_and_records_the_log() {
  // `.txt` 入力で警告を 1 件出し、警告と成功サマリの両方を端末へ書かせる
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.txt", BODY, "out");

  let status = seiran_with_closed_stderr(dir.path(), &["build", "-c", "config.toml", "--log-file", "x.log"]);

  assert_eq!(status.code(), Some(0), "端末へ書けなくても成功した build は終了 0（panic なら 101）");
  assert!(dir.path().join("out/doc.pdf").is_file(), "PDF は保存される");
  let log = fs::read_to_string(dir.path().join("x.log")).expect("ログファイルができているはず");
  assert!(log.contains(SOURCE_EXTENSION_CODE), "警告はログへ残る: {log}");
  assert!(log.contains('\u{2713}'), "成功サマリはログへ残る: {log}");
  assert!(log.contains("終了状態: 成功"), "終了記録は成功: {log}");
}

#[test]
fn closed_terminal_with_verbose_does_not_stop_a_build() {
  // 工程行は tracing の stderr layer が書く
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_ok_project(dir.path());

  let status = seiran_with_closed_stderr(dir.path(), &["build", "-c", "config.toml", "-vvv"]);

  assert_eq!(status.code(), Some(0), "工程行を書けなくても終了 0（panic なら 101）");
  assert!(dir.path().join("out/doc.pdf").is_file(), "PDF は保存される");
}
