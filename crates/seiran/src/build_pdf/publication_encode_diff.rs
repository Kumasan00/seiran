//! 新旧 encode 経路の byte-for-byte 一致を証明する一時テスト（epic #252 step7、issue #265）。
//!
//! `pdf_gen::create_pdf`（旧、`Config`/`Style`/`model::Page` ベース）と
//! `pdf_gen::create_pdf_from_publication`（新、Publication ベース）が同じ入力に対して
//! 完全に同じ PDF バイト列を生成することを、実組版パス（`build_pages`）を通した複数の
//! 代表 fixture で確認する。cutover（旧経路の削除）と同時にこのファイルごと削除する
//! （比較対象の一方が無くなるため存在意義が終わる）。

use std::path::PathBuf;

use font::{FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt};
use pdf_gen::PublicationBuilder;

use super::golden::{enter_workspace_root, load_base};

/// 比較対象入力（`publication_diff.rs` と同じ代表4件: 画像アセット非依存で見出し・表・
/// リンク・脚注を横断する）
const DIFF_INPUTS: &[&str] = &["text", "table", "hyperref", "footnote"];

#[test]
#[allow(clippy::unwrap_used)]
fn new_encode_path_matches_legacy_encode_path_byte_for_byte() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut mismatches: Vec<&str> = Vec::new();

  for name in DIFF_INPUTS {
    let mut config = base_config.clone();
    config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
    let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
    let laid_out = super::build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");
    let font_refs = FontRefs::new(&config.font_configs, &font_data).expect("FontRefs の構築");
    let metrics = FontMetrics::new(&font_refs).expect("FontMetrics の構築");

    // Act
    let legacy_bytes = pdf_gen::create_pdf(
      &config,
      &font_data,
      &font_refs,
      &metrics,
      &laid_out.pages,
      &style,
      &laid_out.outline_entries,
    )
    .expect("旧経路の描画");
    let publication = PublicationBuilder::new(&config).build(&laid_out.pages, &laid_out.outline_entries);
    let new_bytes =
      pdf_gen::create_pdf_from_publication(&publication, &font_data, &font_refs, &metrics, &config.font_configs)
        .expect("新経路の描画");

    // Assert（即 panic せず全 fixture を集めてから一括報告する）
    if legacy_bytes != new_bytes {
      mismatches.push(name);
    }
  }

  assert!(
    mismatches.is_empty(),
    "Publication 経路が旧 renderer と異なる PDF バイト列を生成しました: {mismatches:?}"
  );
}
