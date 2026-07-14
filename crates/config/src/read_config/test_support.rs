//! テスト用フィクスチャ生成ヘルパ
//!
//! `read_config` のユニットテスト（`src/read_config.rs#tests`）と統合テスト（`tests/`）の双方で
//! 共有する TOML テキスト組み立てヘルパを集約します。`#[doc(hidden)] pub` で公開して
//! いますが、利用者向け API ではなくテストインフラ専用です。
//!
//! I/O を伴うヘルパ（例: `tempfile` で実ファイルを配置する `setup_config`）は dev 依存に
//! 限定したいため、統合テスト側（`tests/common/mod.rs`）に残しています。

use std::fmt::Write as _;

use model::FontType;

/// 19 フォント種別すべての `[font_configs.<key>]` セクションを生成します。
///
/// 各セクションは `font_name = "font_<key>"`、`font_path = <font_path>` で埋めます。
/// セクション名は [`FontType::as_toml_key`] と単一情報源を共有するため、フォント種別を
/// 追加してもこちらの更新は不要です。
#[must_use]
pub fn make_font_sections(font_path: &str) -> String {
  let mut out = String::new();
  for font_type in FontType::ALL {
    let key = font_type.as_toml_key();
    write!(out, "[font_configs.{key}]\nfont_name = \"font_{key}\"\nfont_path = \"{font_path}\"\n\n").unwrap();
  }
  return out;
}

/// 妥当な `[output]` セクションを生成します。
#[must_use]
pub fn valid_output_section(name: &str, output_dir: &str) -> String {
  return format!("[output]\nname = \"{name}\"\noutput_dir = \"{output_dir}\"\n\n");
}

/// 妥当な `[pdf]` セクションを生成します（A4 縦・50pt 余白）。
#[must_use]
pub fn valid_pdf_section() -> String {
  return "[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
          margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n"
    .to_string();
}

/// `[font_configs.serif]` セクションに任意のフィールド追加行を差し込んだ TOML を生成します。
///
/// `extra_lines` には `font_name` / `font_path` 以外のフィールド（例: `language = "ja-JP"`）を
/// 改行区切りで指定します。
#[must_use]
pub fn font_sections_with_serif_extra(font_path: &str, extra_lines: &str) -> String {
  let base = make_font_sections(font_path);
  let needle = "[font_configs.serif]\nfont_name = \"font_serif\"\nfont_path = \"";
  let injected = format!("[font_configs.serif]\nfont_name = \"font_serif\"\n{extra_lines}\nfont_path = \"");
  return base.replace(needle, &injected);
}
