//! `read_config` の統合テストで共有するフィクスチャ生成ヘルパ。
//!
//! `tests/` 配下のサブディレクトリは Cargo がテストターゲットとして拾わないため、
//! `mod common;` で各統合テストから取り込んで利用します。

use std::{fmt::Write as _, path::PathBuf};

use tempfile::TempDir;

/// 19 フォント種別すべての設定セクションを生成するヘルパー。
pub fn make_font_sections(font_path: &str) -> String {
  const SECTION_NAMES: [&str; 19] = [
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
  let mut out = String::new();
  for name in SECTION_NAMES {
    write!(out, "[font_configs.{name}]\nfont_name = \"font_{name}\"\nfont_path = \"{font_path}\"\n\n").unwrap();
  }
  return out;
}

/// 既定の `[output]` セクション（妥当な値）を生成します。
pub fn valid_output_section(name: &str, output_dir: &str) -> String {
  return format!("[output]\nname = \"{name}\"\noutput_dir = \"{output_dir}\"\n\n");
}

/// 既定の `[pdf]` セクション（妥当な値）を生成します。
pub fn valid_pdf_section() -> String {
  return "[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
          margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n"
    .to_string();
}

/// `[font_configs.serif]` セクションに任意のフィールド追加行を差し込んだ TOML を生成します。
///
/// `extra_lines` には `font_name` / `font_path` 以外のフィールド（例: `language = "ja-JP"`）を
/// 改行区切りで指定します。
pub fn font_sections_with_serif_extra(font_path: &str, extra_lines: &str) -> String {
  let base = make_font_sections(font_path);
  let needle = "[font_configs.serif]\nfont_name = \"font_serif\"\nfont_path = \"";
  let injected_marker = format!("[font_configs.serif]\nfont_name = \"font_serif\"\n{extra_lines}\nfont_path = \"");
  return base.replace(needle, &injected_marker);
}

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
