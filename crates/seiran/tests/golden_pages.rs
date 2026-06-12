//! 構造ゴールデンテスト — 小さい固定ソースに対する `Vec<Page>` の Debug 表現比較
//!
//! パイプライン（`parse` → `lower` → `build_blocks` → `resolve_images` → `break_pages`）の
//! 出力構造を Debug 表現でゴールデンファイルと比較する。PDF バイナリではなく
//! 構造体を比較するため、krilla / メタデータ（日付等）の影響を受けない。
//!
//! ゴールデンの更新: `UPDATE_GOLDEN=1 cargo test -p seiran --test golden_pages`

use std::{fs, path::PathBuf};

use font::{
  FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
};
use lowering::LoweringContext;

/// ゴールデン対象の固定ソース（見出し + 和欧混在段落 + インライン数式 + 強制改行）
const GOLDEN_SOURCE: &str = "\\section{ゴールデン}

Wrapping の検証 mixed 段落。インライン数式 $x^2$ を含む。\\\\
強制改行後の行。
";

/// ワークスペースルートを返す
fn repo_root() -> PathBuf {
  return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .canonicalize()
    .expect("リポジトリルートがあるはず");
}

#[test]
fn pages_structure_matches_golden() {
  // Arrange — リポジトリの config.toml の相対パスを絶対パスに書き換えて読み込む
  let root = repo_root();
  let root_str = root.display().to_string();
  let config_text = fs::read_to_string(root.join("config/config.toml"))
    .expect("config/config.toml を読めるはず")
    .replace("\"fonts/", &format!("\"{root_str}/fonts/"))
    .replace("\"tests/text/", &format!("\"{root_str}/tests/text/"))
    .replace("\"config/style.toml\"", &format!("\"{root_str}/config/style.toml\""))
    .replace("\"config/references.toml\"", &format!("\"{root_str}/config/references.toml\""));
  let temp_dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let config_path = temp_dir.path().join("config.toml");
  fs::write(&config_path, config_text).expect("一時 config を書けるはず");

  let config = read_config::read_config(&config_path).expect("config を読めるはず");
  let style = read_style::read_style(config.style_path.as_deref()).expect("style を読めるはず");

  // Act — parse → lower → build_blocks → resolve_images → break_pages
  let doc_nodes = parser::parse_source(GOLDEN_SOURCE, "golden.sei", &style).expect("パースできるはず");
  let ctx = LoweringContext::new(&style);
  let layout_nodes = lowering::lower_nodes(&ctx, &doc_nodes).expect("lowering できるはず");

  let font_data = FontData::new(&config.font_configs).expect("フォントを読めるはず");
  let font_refs = FontRefs::new(&config.font_configs, &font_data).expect("フォントを解析できるはず");
  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)
    .expect("シェーパーを作れるはず");
  let metrics = FontMetrics::new(&font_refs).expect("メトリクスを取れるはず");

  let text_width = config.pdf.width.to_pt() - config.pdf.margin.left.to_pt() - config.pdf.margin.right.to_pt();
  let default_font_size = style.core.font_size.to_pt();
  let line_height_factor = style.core.line_height_factor;
  let blocks = layout::build_blocks(layout_nodes, &shapers, &metrics, default_font_size, line_height_factor);
  let blocks = pdf_gen::resolve_images(blocks, text_width).expect("画像なしなので失敗しないはず");
  let geometry = hlist::PageGeometry {
    margin_top: config.pdf.margin.top.to_pt(),
    page_limit: config.pdf.height.to_pt() - config.pdf.margin.bottom.to_pt(),
    default_font_size,
    line_height_factor,
    table_cell_padding: style.core.table.cell_padding.to_pt(),
  };
  let pages = hlist::break_pages(blocks, text_width, &geometry, &hlist::GreedyBreaker);

  // Assert — Debug 表現でゴールデン比較
  let actual = format!("{pages:#?}\n");
  let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pages.txt");
  if std::env::var("UPDATE_GOLDEN").is_ok() {
    fs::create_dir_all(golden_path.parent().expect("親ディレクトリがあるはず")).expect("ディレクトリを作れるはず");
    fs::write(&golden_path, &actual).expect("ゴールデンを書けるはず");
    return;
  }
  let expected =
    fs::read_to_string(&golden_path).expect("ゴールデンファイルがあるはず（UPDATE_GOLDEN=1 で生成してください）");
  assert_eq!(
    actual, expected,
    "Vec<Page> の構造がゴールデンと一致しません（意図した変更なら UPDATE_GOLDEN=1 で更新）"
  );
}
