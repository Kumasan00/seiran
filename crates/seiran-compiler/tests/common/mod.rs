//! 統合テスト間で共有するヘルパ。

use std::path::Path;

use seiran_compiler::test_support;

/// 和文 6 種別に割り当てる和文フォントの、テストプロジェクト内のパス
pub(crate) const JAPANESE_FONT_PATH: &str = "/project/japanese.ttf";

/// `vendor/fonts/` のフォント `file_name` を読む（初回は `tools/fetch-test-assets.sh` の実行が必要）。
fn read_vendor_font(file_name: &str) -> Vec<u8> {
  let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-compiler の 2 階層上がワークスペースルート");
  return std::fs::read(workspace_root.join("vendor/fonts").join(file_name))
    .expect("vendor/fonts のフォントを読めるはず（tools/fetch-test-assets.sh の実行が必要な場合があります）");
}

/// `vendor/fonts/` にある golden テスト用の実フォント（STIX Two Math。MATH を持つので 19 種別すべてに使える）。
pub(crate) fn read_test_font() -> Vec<u8> { return read_vendor_font("STIXTwoMath-Regular.ttf"); }

/// [`JAPANESE_FONT_PATH`] に置く和文フォント（Noto Serif JP。軸 `wght` を持つ）。
pub(crate) fn read_japanese_test_font() -> Vec<u8> { return read_vendor_font("NotoSerifJP[wght].ttf"); }

/// `[font_configs.*]` 群 `font_sections` の和文 6 種別の候補を [`JAPANESE_FONT_PATH`]（`wght` 400）に差し替える。
///
/// 本文の和文は和文の枠で組むので、数式フォントだけの構成では全候補に字形が無いエラーになる。
pub(crate) fn with_japanese_slots(font_sections: &str) -> String {
  return font_sections
    .split_inclusive("\n\n")
    .map(|block| {
      if !block.starts_with("[[font_configs.japanese_") {
        return block.to_string();
      }
      let header = block.lines().next().expect("split_inclusive は空でない塊を返す");
      return format!(
        "{header}\nfont_path = \"{JAPANESE_FONT_PATH}\"\nvariation_axes = [{{ name = \"wght\", value = 400.0 }}]\n\n"
      );
    })
    .collect();
}

/// 妥当な `[pdf]` / `[output]` に任意の `[font_configs.*]` 群を足して `config.toml` を組む。
///
/// `extra_top_level` は `sources` の直後へ差し込むトップレベル行（末尾の改行込み。不要なら空文字）。
pub(crate) fn config_toml_with_font_sections(source_path: &str, extra_top_level: &str, font_sections: &str) -> String {
  return format!(
    "sources = [\"{source_path}\"]\n{extra_top_level}\n{}{}{font_sections}",
    test_support::valid_pdf_section(),
    test_support::valid_output_section("out", "/project/out"),
  );
}

/// 和文 6 種別が [`JAPANESE_FONT_PATH`]、残り 13 種別が同じフォントファイル `/project/font.ttf` を指す、最小の妥当な
/// `config.toml` を組む。
pub(crate) fn minimal_config_toml(source_path: &str) -> String {
  return config_toml_with_font_sections(
    source_path,
    "",
    &with_japanese_slots(&test_support::font_sections("/project/font.ttf")),
  );
}
