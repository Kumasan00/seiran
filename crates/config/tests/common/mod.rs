//! 統合テスト専用のフィクスチャ生成ヘルパ
//!
//! 純文字列ヘルパ（`make_font_sections` 等）は `config::read_config::test_support` に集約済みで、
//! ここではユニットテストでは不要な「実ファイル配置を伴う」ヘルパだけを保持します。
//! `tempfile` を dev-dependency に閉じ込めるための分離です。

use std::path::PathBuf;

use tempfile::TempDir;

/// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します。
///
/// `build_toml(font_path, output_dir, source_path)` の各引数は絶対パス文字列で、
/// テスト側はこれらを TOML テキストの組み立てに使う。
pub fn setup_config(build_toml: impl FnOnce(&str, &str, &str) -> String) -> (TempDir, PathBuf) {
  let tempdir = tempfile::tempdir().unwrap();
  let font_path = tempdir.path().join("dummy.ttf");
  std::fs::write(&font_path, b"").unwrap();
  let source_path = tempdir.path().join("source.sei");
  std::fs::write(&source_path, b"").unwrap();
  let output_dir = tempdir.path().join("output");
  let config_path = tempdir.path().join("config.toml");
  let toml_text = build_toml(font_path.to_str().unwrap(), output_dir.to_str().unwrap(), source_path.to_str().unwrap());
  std::fs::write(&config_path, toml_text).unwrap();
  return (tempdir, config_path);
}
