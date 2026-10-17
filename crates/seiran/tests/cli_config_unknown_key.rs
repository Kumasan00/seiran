//! config.toml の未知キーで `build` が止まり、出力先の既存 PDF に触れないことを、binary を起動して確かめる
//!
//! 各階層の未知キーの拒否そのものは `seiran-compiler` の `project::config` が覆う。ここでは CLI の終了コードと、
//! 拒否された実行が既存の同名 PDF を上書きしないことを見る。

mod common;

use std::fs;

use common::{seiran, stderr_text, write_project};

/// 未知キーを含む config.toml の TOML 解析失敗の診断 code。
const PARSE_TOML_CODE: &str = "project::config::parse_toml";

/// 出力先に先に置く、PDF ではない中身。
const SENTINEL: &[u8] = b"sentinel";

/// `edit` で config.toml に未知キーを足して `build` し、終了コード 1・診断・既存 PDF の不変を確かめる。
fn assert_build_rejects(edit: impl FnOnce(&str) -> String) {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.sei", "Hello, Seiran!", "out");
  let config_path = dir.path().join("config.toml");
  let config = fs::read_to_string(&config_path).expect("config.toml を読めるはず");
  fs::write(&config_path, edit(&config)).expect("config.toml を書けるはず");
  fs::create_dir(dir.path().join("out")).expect("出力ディレクトリを作れるはず");
  let pdf_path = dir.path().join("out/doc.pdf");
  fs::write(&pdf_path, SENTINEL).expect("既存 PDF を置けるはず");

  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "未知キーは処理失敗: {stderr}");
  assert!(stderr.contains(PARSE_TOML_CODE), "{stderr}");
  assert!(stderr.contains("unknown field `unknown_documentation_probe`"), "{stderr}");
  assert!(stderr.contains("config.toml"), "{stderr}");
  assert_eq!(fs::read(&pdf_path).expect("既存 PDF は残るはず"), SENTINEL, "既存 PDF を上書きしないはず");
}

#[test]
fn unknown_top_level_key_fails_without_touching_the_existing_pdf() {
  assert_build_rejects(|config| return format!("unknown_documentation_probe = true\n{config}"));
}

#[test]
fn unknown_output_key_fails_without_touching_the_existing_pdf() {
  assert_build_rejects(|config| {
    return config.replacen("[output]\n", "[output]\nunknown_documentation_probe = true\n", 1);
  });
}
