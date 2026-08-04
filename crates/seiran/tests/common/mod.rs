//! 統合テスト間で共有するヘルパ（`tests/common/mod.rs` は Rust の慣例でテストファイルとして
//! 扱われないため、共有ヘルパの置き場所として使う）。

use std::path::Path;

use config::test_support;

/// `vendor/fonts/` にある golden テスト用の実フォント（他の golden テストと共有する資産。
/// 初回は `tools/fetch-test-assets.sh` の実行が必要 — CI はキャッシュ済みかここで取得する）。
pub fn read_test_font() -> Vec<u8> {
  let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran の 2 階層上がワークスペースルート");
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
