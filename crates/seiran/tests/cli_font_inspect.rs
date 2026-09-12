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

/// フォントのテーブルディレクトリから `tag` のレコードの位置（ファイル先頭からのバイト位置）を探す。
///
/// sfnt のヘッダは 12 バイトで、numTables が 4〜5 バイト目、以後 16 バイトのレコード（tag / checksum /
/// offset / length）が並ぶ。
fn table_record_position(font: &[u8], tag: [u8; 4]) -> usize {
  let table_count = usize::from(u16::from_be_bytes([font[4], font[5]]));
  return (0..table_count)
    .map(|index| return 12 + 16 * index)
    .find(|&position| return font[position..position + 4] == tag)
    .expect("テストに使うフォントには対象テーブルのレコードがあるはず");
}

/// `tag` のテーブルの先頭位置（ファイル先頭からのバイト位置）を返す。
fn table_offset(font: &[u8], tag: [u8; 4]) -> usize {
  let record = table_record_position(font, tag);
  let offset = u32::from_be_bytes([
    font[record + 8],
    font[record + 9],
    font[record + 10],
    font[record + 11],
  ]);
  return usize::try_from(offset).expect("テーブルの位置は usize に収まる");
}

/// `source` を `dir/name` へコピーし、`patch` でバイト列を書き換える。
fn write_patched_copy(source: &Path, dir: &Path, name: &str, patch: impl FnOnce(&mut [u8])) {
  let mut bytes = fs::read(source).expect("フォントを読めるはず");
  patch(&mut bytes);
  fs::write(dir.join(name), bytes).expect("書き換えたフォントを書けるはず");
}

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

#[test]
fn variation_axes_reports_a_font_without_fvar_as_not_variable() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let font = vendor_font("STIXTwoMath-Regular.ttf");

  let output = seiran(dir.path(), &["variation-axes", path_arg(&font)]);

  assert_eq!(output.status.code(), Some(0), "fvar が無いのは正常: {}", stderr_text(&output));
  assert_eq!(stdout_text(&output), "The font is not a variable font.\n");
}

#[test]
fn variation_axes_lists_axes_and_instances_of_a_variable_font() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let font = vendor_font("NotoSans[wdth,wght].ttf");

  let output = seiran(dir.path(), &["variation-axes", path_arg(&font)]);

  assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
  let stdout = stdout_text(&output);
  assert!(
    stdout.starts_with(
      "Axis: wght, Min: 100, Default: 400, Max: 900\nAxis: wdth, Min: 62.5, Default: 100, Max: 100\nThin: [100.0, 100.0]\n"
    ),
    "軸 → インスタンスの順で現行と同じ書式: {stdout}"
  );
  assert_eq!(stdout.lines().count(), 11, "軸 2 本とインスタンス 9 件: {stdout}");
}

#[test]
fn variation_axes_rejects_a_broken_fvar() {
  // Arrange — fvar のレコードの length だけを 1 にする（#549 の再現手順 1。他のテーブルは無傷）
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_patched_copy(&vendor_font("NotoSans[wdth,wght].ttf"), dir.path(), "broken.ttf", |font| {
    let record = table_record_position(font, *b"fvar");
    font[record + 12..record + 16].copy_from_slice(&1u32.to_be_bytes());
  });

  // Act
  let output = seiran(dir.path(), &["variation-axes", "broken.ttf"]);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "壊れた fvar は「可変フォントではない」にしない: {stderr}");
  assert!(stderr.contains("cli::variation_axes::fvar"), "fvar 破損の診断: {stderr}");
  assert!(stderr.contains("broken.ttf"), "対象パスが出る: {stderr}");
  assert_eq!(stderr.matches("out of bounds").count(), 1, "解析エラーの文は cause に 1 回だけ: {stderr}");
  assert!(stdout_text(&output).is_empty(), "一覧は 1 行も出さない");
}

#[test]
fn variation_axes_rejects_an_fvar_record_whose_length_runs_past_the_file() {
  // Arrange — fvar のレコードの length をファイルの範囲外まで伸ばす
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_patched_copy(&vendor_font("NotoSans[wdth,wght].ttf"), dir.path(), "range.ttf", |font| {
    let record = table_record_position(font, *b"fvar");
    font[record + 12..record + 16].copy_from_slice(&0xffff_fff0u32.to_be_bytes());
  });

  // Act
  let output = seiran(dir.path(), &["variation-axes", "range.ttf"]);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "範囲外を指す fvar は「可変フォントではない」にしない: {stderr}");
  assert!(stderr.contains("cli::variation_axes::fvar_range"), "範囲外の診断: {stderr}");
  assert!(stderr.contains("range.ttf"), "対象パスが出る: {stderr}");
  assert!(!stderr.contains("table is missing"), "read-fonts の cause 文言を出さない: {stderr}");
  assert!(stdout_text(&output).is_empty(), "一覧は 1 行も出さない");
}

#[test]
fn variation_axes_rejects_an_fvar_record_whose_offset_is_zero() {
  // Arrange — fvar のレコードの offset を 0 にする（レコード先頭 + 8..12）
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_patched_copy(&vendor_font("NotoSans[wdth,wght].ttf"), dir.path(), "zero_offset.ttf", |font| {
    let record = table_record_position(font, *b"fvar");
    font[record + 8..record + 12].copy_from_slice(&0u32.to_be_bytes());
  });

  // Act
  let output = seiran(dir.path(), &["variation-axes", "zero_offset.ttf"]);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "オフセット 0 の fvar は「可変フォントではない」にしない: {stderr}");
  assert!(stderr.contains("cli::variation_axes::fvar_range"), "範囲外の診断: {stderr}");
  assert!(stderr.contains("zero_offset.ttf"), "対象パスが出る: {stderr}");
  assert!(!stderr.contains("table is missing"), "read-fonts の cause 文言を出さない: {stderr}");
  assert!(stdout_text(&output).is_empty(), "一覧は 1 行も出さない");
}

#[test]
fn variation_axes_rejects_truncated_instances() {
  // Arrange — fvar ヘッダの instanceCount（テーブル先頭から 12 バイト目）を実際より大きくする
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_patched_copy(&vendor_font("NotoSans[wdth,wght].ttf"), dir.path(), "truncated.ttf", |font| {
    let fvar = table_offset(font, *b"fvar");
    font[fvar + 12..fvar + 14].copy_from_slice(&0xffffu16.to_be_bytes());
  });

  // Act
  let output = seiran(dir.path(), &["variation-axes", "truncated.ttf"]);

  // Assert
  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "インスタンスを黙って落とさない: {stderr}");
  assert!(stderr.contains("cli::variation_axes::truncated_records"), "切り詰めの診断: {stderr}");
  assert!(stderr.contains("truncated.ttf"), "対象パスが出る: {stderr}");
}

#[test]
fn variation_axes_survives_a_closed_reader() {
  let font = vendor_font("NotoSans[wdth,wght].ttf");

  let output = seiran_with_closed_stdout(&["variation-axes", path_arg(&font)]);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "受け手の終了は正常終了: {stderr}");
  assert!(!stderr.contains("panicked"), "panic しない: {stderr}");
}

#[test]
fn variation_axes_reports_a_missing_file_with_its_path() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

  let output = seiran(dir.path(), &["variation-axes", "missing.ttf"]);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "{stderr}");
  assert!(stderr.contains("cli::variation_axes::read_file"), "読み込み失敗の診断: {stderr}");
  assert!(stderr.contains("missing.ttf"), "対象パスが出る: {stderr}");
  assert_eq!(stderr.matches("os error 2").count(), 1, "OS エラー文は cause に 1 回だけ: {stderr}");
}

#[test]
fn script_langs_reports_a_missing_file_with_its_path() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

  let output = seiran(dir.path(), &["script-langs", "missing.ttf"]);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "{stderr}");
  assert!(stderr.contains("cli::script_langs::read_file"), "読み込み失敗の診断: {stderr}");
  assert!(stderr.contains("missing.ttf"), "対象パスが出る: {stderr}");
  assert_eq!(stderr.matches("os error 2").count(), 1, "OS エラー文は cause に 1 回だけ: {stderr}");
}

#[test]
fn script_langs_survives_a_closed_reader() {
  let font = vendor_font("NotoSans[wdth,wght].ttf");

  let output = seiran_with_closed_stdout(&["script-langs", path_arg(&font)]);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(0), "受け手の終了は正常終了: {stderr}");
  assert!(!stderr.contains("panicked"), "panic しない: {stderr}");
}

#[test]
fn script_langs_ends_with_feature_statistics() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let font = vendor_font("STIXTwoMath-Regular.ttf");

  let output = seiran(dir.path(), &["script-langs", path_arg(&font)]);

  assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
  let stdout = stdout_text(&output);
  assert!(stdout.starts_with("GSUB Table:\n"), "GSUB から始まる: {stdout}");
  assert!(
    stdout.ends_with(
      "\nFeature Statistics:\n  Total Features in GSUB/GPOS: 29\n  Referenced in Script/Language Systems: 29\n  Unreferenced Features: []\n"
    ),
    "空行を挟んで統計で終わる（現行と同じ書式）: {stdout}"
  );
}

/// `/dev/full` は Linux にしかない（CI は ubuntu で走る）。
#[cfg(target_os = "linux")]
#[test]
fn unwritable_stderr_does_not_turn_a_failure_into_a_panic() {
  // Arrange
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let dev_full = fs::OpenOptions::new().write(true).open("/dev/full").expect("/dev/full を開けるはず");

  // Act — 失敗する実行の診断を、書けない stderr へ出させる
  let output = Command::new(env!("CARGO_BIN_EXE_seiran"))
    .args(["ttc-names", "missing.ttc"])
    .current_dir(dir.path())
    .env_remove("RUST_LOG")
    .stderr(dev_full)
    .output()
    .expect("seiran を起動できるはず");

  // Assert
  assert_eq!(output.status.code(), Some(1), "報告を書けなくても処理失敗の終了コード 1（panic の 101 ではない）");
}
