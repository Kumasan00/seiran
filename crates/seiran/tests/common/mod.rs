//! CLI 統合テスト間で共有するヘルパ（`tests/common/mod.rs` は Rust の慣例でテストファイルとして
//! 扱われないため、共有ヘルパの置き場所として使う）。
//!
//! ここに置く項目は、`mod common;` するすべてのテストファイルが使うものだけにする — 各テストファイルは
//! 別 crate としてコンパイルされるので、1 ファイルでも使わない項目は `dead_code` になる。

use std::{
  fs,
  path::{Path, PathBuf},
  process::{Command, Output},
};

use seiran_compiler::test_support;

/// テスト対象の binary を `dir` をカレントディレクトリにして起動する。
///
/// `rust_log` が `None` なら、開発者の shell の `RUST_LOG` で stderr とファイルの内容が揺れないよう外す。
/// `Some` ならその値を `RUST_LOG` に設定する。
pub(crate) fn seiran(dir: &Path, args: &[&str], rust_log: Option<&str>) -> Output {
  let mut command = Command::new(env!("CARGO_BIN_EXE_seiran"));
  command.args(args).current_dir(dir);
  match rust_log {
    Some(value) => command.env("RUST_LOG", value),
    None => command.env_remove("RUST_LOG"),
  };
  return command.output().expect("seiran を起動できるはず");
}

/// `Output` の stderr を文字列にする。
pub(crate) fn stderr_text(output: &Output) -> String { return String::from_utf8_lossy(&output.stderr).into_owned(); }

/// テストフォント（`vendor/fonts/STIXTwoMath-Regular.ttf`、`tools/fetch-test-assets.sh` で取得）の絶対パスを返す。
fn test_font() -> PathBuf {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
  assert!(
    path.is_file(),
    "{} を読めるはず（tools/fetch-test-assets.sh の実行が必要な場合があります）",
    path.display()
  );
  return path;
}

/// `source_name` を唯一のソースにし、PDF を `output_dir`（`dir` からの相対）へ出す config.toml と本文を `dir` へ書く。
///
/// フォントはテストフォントを 19 種別すべてに使う。拡張子が `.sei` でない `source_name` は拡張子の警告を 1 件生む。
pub(crate) fn write_project(dir: &Path, source_name: &str, body: &str, output_dir: &str) {
  let font = test_font();
  let config = format!(
    "sources = [\"{source_name}\"]\n\n{}{}{}",
    test_support::valid_output_section("doc", output_dir),
    test_support::valid_pdf_section(),
    test_support::make_font_sections(font.to_str().expect("テストフォントのパスは UTF-8 のはず")),
  );
  fs::write(dir.join("config.toml"), config).expect("config.toml を書けるはず");
  fs::write(dir.join(source_name), body).expect("本文を書けるはず");
}
