//! 統合テスト間で共有するヘルパ（`tests/common/mod.rs` は Rust の慣例でテストファイルとして
//! 扱われないため、共有ヘルパの置き場所として使う）。

use std::path::Path;

use seiran_compiler::test_support;

/// `vendor/fonts/` にある golden テスト用の実フォント（他の golden テストと共有する資産。
/// 初回は `tools/fetch-test-assets.sh` の実行が必要 — CI はキャッシュ済みかここで取得する）。
pub fn read_test_font() -> Vec<u8> {
  let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-compiler の 2 階層上がワークスペースルート");
  let path = workspace_root.join("vendor/fonts/STIXTwoMath-Regular.ttf");
  return std::fs::read(&path).unwrap_or_else(|error| {
    panic!(
      "テストフォントを読めるはず: {}: {error}（tools/fetch-test-assets.sh の実行が必要な場合があります）",
      path.display()
    )
  });
}

/// 19 フォント種別すべてが同じフォントファイルを指す、最小の妥当な `config.toml` を組む。
pub fn minimal_config_toml(source_path: &str) -> String {
  return format!(
    "sources = [\"{source_path}\"]\n\n{}{}{}",
    test_support::valid_pdf_section(),
    test_support::valid_output_section("out", "/project/out"),
    test_support::make_font_sections("/project/font.ttf"),
  );
}

/// [`minimal_config_toml`] の `[font_configs.serif]` に任意の行を足した `config.toml` を組む。
///
/// `tests/common/mod.rs` は 2 つのテストバイナリへ別々にコンパイルされるので、片方でしか
/// 使わないヘルパは `dead_code` になる（慣例どおり `allow` で許容する）。
#[allow(dead_code)]
pub fn minimal_config_toml_with_serif_extra(source_path: &str, extra_lines: &str) -> String {
  return format!(
    "sources = [\"{source_path}\"]\n\n{}{}{}",
    test_support::valid_pdf_section(),
    test_support::valid_output_section("out", "/project/out"),
    test_support::font_sections_with_serif_extra("/project/font.ttf", extra_lines),
  );
}

/// [`minimal_config_toml_with_serif_extra`] に `style_path` を足した `config.toml` を組む。
///
/// `extra_lines` に空文字を渡せば `[font_configs.serif]` は既定のままになる。
#[allow(dead_code)]
pub fn config_toml_with_style(source_path: &str, style_path: &str, extra_lines: &str) -> String {
  return format!(
    "sources = [\"{source_path}\"]\nstyle_path = \"{style_path}\"\n\n{}{}{}",
    test_support::valid_pdf_section(),
    test_support::valid_output_section("out", "/project/out"),
    test_support::font_sections_with_serif_extra("/project/font.ttf", extra_lines),
  );
}

/// 脚注 1 行がページの高さを超える `style.toml` を組む（組版警告の再現用）。
///
/// `numbering` は `"continuous"` / `"per_page"`（後者は本文パスが不動点まで反復される）。
#[allow(dead_code)]
pub fn overflowing_footnote_style_toml(numbering: &str) -> String {
  return format!("[footnote]\nnumbering = \"{numbering}\"\nfont_size = \"900pt\"\n");
}
