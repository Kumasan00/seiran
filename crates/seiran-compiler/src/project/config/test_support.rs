//! テスト用フィクスチャ生成ヘルパ

use std::fmt::Write as _;

use strum::VariantArray;

use crate::project::FontType;

/// 19 フォント種別すべてのセクション（本文用は候補 1 つの `[[font_configs.<key>]]`、数式は `[font_configs.math]`）を生成します。
#[must_use]
pub fn font_sections(font_path: &str) -> String {
  let mut out = String::new();
  for &font_type in FontType::VARIANTS {
    let key = font_type.as_toml_key();
    let header = if font_type == FontType::Math {
      format!("[font_configs.{key}]")
    } else {
      format!("[[font_configs.{key}]]")
    };
    write!(out, "{header}\nfont_path = \"{font_path}\"\n\n")
      .expect("`String` への `write!` は `fmt::Error` を返さない");
  }
  return out;
}

/// 妥当な `[output]` セクションを生成します。
#[must_use]
pub fn valid_output_section(name: &str, output_dir: &str) -> String {
  return format!("[output]\nname = \"{name}\"\noutput_dir = \"{output_dir}\"\n\n");
}

/// 妥当な `[pdf]` セクションを生成します（A4 縦）。
#[must_use]
pub fn valid_pdf_section() -> String { return "[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\n".to_string(); }

/// `serif` の先頭の候補（`[[font_configs.serif]]` の 1 つ目）に任意のフィールド追加行を差し込んだ TOML を生成します。
#[must_use]
pub fn font_sections_with_serif_extra(font_path: &str, extra_lines: &str) -> String {
  let base = font_sections(font_path);
  let needle = "[[font_configs.serif]]\nfont_path = \"";
  let injected = format!("[[font_configs.serif]]\n{extra_lines}\nfont_path = \"");
  return base.replacen(needle, &injected, 1);
}

/// [`font_sections`] の `serif` に 2 つ目の候補を足した TOML を生成します。
///
/// `candidate_lines` は 2 つ目の候補の本体（`font_path` を含むキーの行）で、先頭の候補の直後に置きます。
#[must_use]
pub fn font_sections_with_serif_candidate(font_path: &str, candidate_lines: &str) -> String {
  let first = format!("[[font_configs.serif]]\nfont_path = \"{font_path}\"\n\n");
  let with_second = format!("{first}[[font_configs.serif]]\n{candidate_lines}\n\n");
  return font_sections(font_path).replacen(&first, &with_second, 1);
}
