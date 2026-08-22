//! 統合テスト間で共有するヘルパ（`tests/common/mod.rs` は Rust の慣例でテストファイルとして
//! 扱われないため、共有ヘルパの置き場所として使う）。

use std::path::Path;

use seiran_compiler::test_support;

/// `vendor/fonts/` にある golden テスト用の実フォント（他の golden テストと共有する資産。
/// 初回は `tools/fetch-test-assets.sh` の実行が必要 — CI はキャッシュ済みかここで取得する）。
pub(crate) fn read_test_font() -> Vec<u8> {
  let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-compiler の 2 階層上がワークスペースルート");
  let path = workspace_root.join("vendor/fonts/STIXTwoMath-Regular.ttf");
  return std::fs::read(&path).expect(
    "vendor/fonts/STIXTwoMath-Regular.ttf を読めるはず（tools/fetch-test-assets.sh の実行が必要な場合があります）",
  );
}

/// 妥当な `[pdf]` / `[output]` に任意の `[font_configs.*]` 群を足して `config.toml` を組む。
///
/// `extra_top_level` は `sources` の直後へ差し込むトップレベル行（末尾の改行込み。不要なら空文字）。
/// `font_sections` の作り分けはこの関数の呼び出し側が担う。
pub(crate) fn config_toml_with_font_sections(source_path: &str, extra_top_level: &str, font_sections: &str) -> String {
  return format!(
    "sources = [\"{source_path}\"]\n{extra_top_level}\n{}{}{font_sections}",
    test_support::valid_pdf_section(),
    test_support::valid_output_section("out", "/project/out"),
  );
}

/// 19 フォント種別すべてが同じフォントファイルを指す、最小の妥当な `config.toml` を組む。
pub(crate) fn minimal_config_toml(source_path: &str) -> String {
  return config_toml_with_font_sections(source_path, "", &test_support::make_font_sections("/project/font.ttf"));
}
