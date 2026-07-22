//! `PublicationBuilder`（`pdf_gen`）と旧 renderer（`pdf_gen::create_pdf` 経由の実 PDF）を
//! 同じ入力で比較する differential test（issue #261 / epic #252 step 5）。
//!
//! `PublicationBuilder` 自体の座標・描画順の正しさは `pdf_gen::publication` のインライン
//! ユニットテスト（手書き期待値との厳密一致、非循環）が担う。ここでは `lopdf` で読み返した
//! 実 PDF の構造的事実（ページ数・リンク注釈数・しおり有無）と `Publication` の対応する事実が
//! 一致することだけを確認する — `Publication` はフォントを直接モデル化しないため
//! `embedded_font_count` は比較対象にしない。
//!
//! `Publication` が実 encode にまだ使われない段階（`pdf_gen::create_pdf` は変更していない）
//! なので、「新旧 2 つの実 renderer」ではなく「実 renderer の出力 vs Publication の自己申告」を
//! 比較する形になる。座標・順序レベルの正しさはユニットテストが担うため、このテストが
//! 拾うのは「ページ・リンク・しおり全体を丸ごと落とす／二重に作る」規模の欠落である。

use std::path::PathBuf;

use font::{FontData, FontDataExt};
use pdf_gen::PublicationBuilder;

use super::{
  golden::{enter_workspace_root, load_base},
  pdf_structure::{build_pdf_bytes, compute_pdf_structure_facts},
};

/// differential test の対象入力（`tests/text/` 配下、画像アセット非依存の代表 4 件）
const DIFF_INPUTS: &[&str] = &["text", "table", "hyperref", "footnote"];

#[test]
#[allow(clippy::unwrap_used)]
fn publication_matches_old_renderer_structure() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut mismatches: Vec<String> = Vec::new();

  for name in DIFF_INPUTS {
    let mut config = base_config.clone();
    config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
    let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
    let laid_out = super::build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");

    // Act
    let publication = PublicationBuilder::new(&config).build(&laid_out.pages, &laid_out.outline_entries);
    let facts = compute_pdf_structure_facts(&build_pdf_bytes(name));

    // Assert（即 panic せず全入力を集めてから一括報告する)
    if publication.pages.len() != facts.page_count {
      mismatches.push(format!("{name}: page_count publication={} pdf={}", publication.pages.len(), facts.page_count));
    }
    let publication_link_count: usize = publication.pages.iter().map(|page| return page.links.len()).sum();
    if publication_link_count != facts.link_annotation_count {
      mismatches.push(format!(
        "{name}: link_count publication={publication_link_count} pdf={}",
        facts.link_annotation_count
      ));
    }
    if publication.outline.is_some() != facts.has_outline {
      mismatches.push(format!(
        "{name}: has_outline publication={} pdf={}",
        publication.outline.is_some(),
        facts.has_outline
      ));
    }
  }

  assert!(mismatches.is_empty(), "Publication と実 PDF 構造が一致しません: {mismatches:?}");
}
