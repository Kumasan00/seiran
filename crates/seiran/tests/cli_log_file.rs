//! `--log-file` への致命的エラー診断の記録を、binary を起動して端から端まで確かめる（#502）
//!
//! `seiran` crate の単体テストは純粋関数（フィルタ計画・診断の描画）を覆うが、「`-q --log-file` で端末は
//! 無言のままファイルに失敗理由が残る」「`--log-file` の有無で stderr と終了コードが変わらない」は `main` の
//! 構造（`Err` を返す直前の記録と、`Termination` による描画の順序）にかかるので、プロセスとして実行して見る。

use std::{
  fmt::Write,
  fs,
  path::{Path, PathBuf},
  process::{Command, Output},
};

/// config.toml が指定を要求する 19 フォント種別。
const FONT_TYPES: [&str; 19] = [
  "serif",
  "serif_bold",
  "serif_italic",
  "serif_bold_italic",
  "sans_serif",
  "sans_serif_bold",
  "sans_serif_italic",
  "sans_serif_bold_italic",
  "monospace",
  "monospace_bold",
  "monospace_italic",
  "monospace_bold_italic",
  "math",
  "japanese_serif",
  "japanese_serif_bold",
  "japanese_sans_serif",
  "japanese_sans_serif_bold",
  "japanese_monospace",
  "japanese_monospace_bold",
];

/// 存在しない config パスの失敗に付く診断 code。
const MISSING_CONFIG_CODE: &str = "project::config::read_file";

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

/// garde 違反を 2 つ（`sources` が空・`keywords` に空要素）持つ config.toml を `dir` へ書き、そのパスを返す。
///
/// 設定値の検証はフォント読込より前に走り、違反は `Failures` として 1 度に返るので、フォント資産が無くても
/// 複数 leaf の `CompileFailure` を作れる。
fn write_config_with_two_violations(dir: &Path) -> PathBuf {
  let mut toml = String::from(
    "sources = []\n\n[document]\nkeywords = [\"\"]\n\n[output]\nname = \"doc\"\n\n[pdf]\nwidth = \"210mm\"\nheight = \
     \"297mm\"\n",
  );
  for font_type in FONT_TYPES {
    write!(toml, "\n[font_configs.{font_type}]\nfont_name = \"{font_type}\"\nfont_path = \"font.ttf\"\n")
      .expect("String への書き込みは失敗しないはず");
  }
  fs::write(dir.join("font.ttf"), b"").expect("ダミーのフォントファイルを書けるはず");
  let path = dir.join("config.toml");
  fs::write(&path, toml).expect("config.toml を書けるはず");
  return path;
}

#[test]
fn failure_diagnostic_is_recorded_in_the_log_file() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let log_path = dir.path().join("x.log");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "missing.toml", "--log-file", "x.log"]);

  // Assert
  assert!(!output.status.success(), "失敗する入力なので終了コードは非 0");
  let log = fs::read_to_string(&log_path).expect("ログファイルができているはず");
  assert!(log.contains(MISSING_CONFIG_CODE), "診断の code が残る: {log}");
  assert!(log.contains("missing.toml"), "診断の本文（パス）が残る: {log}");
  assert!(log.contains("help:"), "診断の help が残る: {log}");
  assert!(!log.contains('\u{1b}'), "ファイルには ANSI 装飾を入れない: {log}");
}

#[test]
fn source_position_is_recorded_in_the_log_file() {
  // Arrange — TOML の構文エラーは位置付きの診断になる
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  fs::write(dir.path().join("bad.toml"), "x = \n").expect("壊れた config.toml を書けるはず");
  let log_path = dir.path().join("x.log");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "bad.toml", "--log-file", "x.log"]);

  // Assert
  assert!(!output.status.success());
  let log = fs::read_to_string(&log_path).expect("ログファイルができているはず");
  assert!(log.contains("project::config::parse_toml"), "診断の code が残る: {log}");
  // `-c` の相対パスは base_dir（起動時のカレントディレクトリ）を基準に解決してから読むので（#530）、
  // ソース位置ブロックにはファイル名の前に絶対パスが付く。ここではプレフィックスを固定せず、
  // ブロック自体とファイル名部分が残っていることだけを見る。
  assert!(log.contains("╭─["), "ソース位置（ファイル名と行・桁）のブロックが残る: {log}");
  assert!(log.contains("bad.toml:"), "ソース位置のファイル名部分が残る: {log}");
}

#[test]
fn quiet_keeps_the_terminal_silent_but_records_the_failure() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let log_path = dir.path().join("x.log");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "missing.toml", "-q", "--log-file", "x.log"]);

  // Assert — `-q` が黙らせるのは端末の非エラー出力だけで、致命的エラーは端末にも現状どおり出る
  assert!(!output.status.success());
  let stderr = stderr_text(&output);
  assert!(stderr.contains(MISSING_CONFIG_CODE), "端末には miette の描画が 1 回出る: {stderr}");
  let log = fs::read_to_string(&log_path).expect("ログファイルができているはず");
  assert!(log.contains(MISSING_CONFIG_CODE), "-q でもファイルには失敗理由が残る: {log}");
}

#[test]
fn log_file_does_not_change_stderr_or_exit_code() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

  // Act
  let without = seiran(dir.path(), &["build", "-c", "missing.toml"]);
  let with = seiran(dir.path(), &["build", "-c", "missing.toml", "--log-file", "x.log"]);

  // Assert
  assert!(!without.status.success());
  assert_eq!(with.status.code(), without.status.code(), "終了コードは --log-file の有無で変わらない");
  assert_eq!(with.stderr, without.stderr, "stderr のバイト列は --log-file の有無で変わらない");
  assert!(dir.path().join("x.log").exists(), "--log-file 指定時はファイルができる");
}

#[test]
fn all_leaves_of_an_aggregated_failure_are_recorded() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let config_path = write_config_with_two_violations(dir.path());
  let log_path = dir.path().join("x.log");

  // Act
  let output = seiran(
    dir.path(),
    &[
      "build",
      "-c",
      config_path.to_str().expect("一時ディレクトリのパスは UTF-8 のはず"),
      "-q",
      "--log-file",
      "x.log",
    ],
  );

  // Assert
  assert!(!output.status.success());
  let log = fs::read_to_string(&log_path).expect("ログファイルができているはず");
  assert!(log.contains("sources は最低 1 つ"), "主診断が残る: {log}");
  assert!(log.contains("keywords[0] は空にできません"), "関連診断（2 件目の leaf）も残る: {log}");
}

#[test]
fn unopenable_log_file_reports_to_the_terminal_only() {
  // Arrange — ディレクトリはファイルとして開けない
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let log_dir = dir.path().join("logs");
  fs::create_dir(&log_dir).expect("ディレクトリを作れるはず");

  // Act
  let output = seiran(dir.path(), &["build", "-c", "missing.toml", "--log-file", "logs"]);

  // Assert — ログファイルが無いので記録しようがなく、端末だけに診断が出て終了コードは非 0
  assert!(!output.status.success());
  let stderr = stderr_text(&output);
  assert!(stderr.contains("cli::open_log_file"), "端末にはログファイルを開けなかった診断が出る: {stderr}");
  assert!(!stderr.contains(MISSING_CONFIG_CODE), "ログファイルを開けなければビルドへ進まない: {stderr}");
  assert!(log_dir.is_dir(), "指定したパスはディレクトリのまま");
}
