//! 組版パス統合 module — 意味解析の成果物（`resolve::AnalyzedDocument`）と CSL 整形の生成物から
//! 計測済み・配置済みページ直前までを担う（旧 `typeset` crate、#307 で `seiran` の非公開 module として吸収）
//!
//! 組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）は本 module 非公開の
//! 子 module `layout` が所有する（#280）。

mod block;
mod breaking;
mod layout;
mod lowering;
mod pipeline;

pub use block::{
  IndexEntryInput, IndexPageRef, RunningContentSpec, RunningMetadata, RunningSlots, TocEntryInput,
  layout_running_content, sort_index_entries,
};
pub use breaking::{KnuthPlassBreaker, PageGeometry};
// `HBox` / `Line` / `Placed*` / `PositionedBox` / `TableCellBox` / `TableRowBox` /
// `measure_items_width` を facade に置いているのは、`build_pdf` 配下の `#[cfg(test)] mod tests`
// が組版済みページを組み立てるのにこれらを名指しするため。`AnchorId` / `AnchorMark` /
// `LinkTarget` / `TableColumn` は `build_pdf::publication` が本体コードから名指しする（#334）。
// `layout` は `typeset` 非公開の子 module なので、facade を通す以外に crate 内から届く経路がない。
// 逆に `Align` / `FootnoteId` は `typeset` の外に消費者がいないので facade へは出さない（#326）。
#[allow(unused_imports)]
pub use layout::{
  AnchorId, AnchorMark, Block, HBox, HBoxContent, HItem, Line, LinkTarget, Page, PlacedAnchor, PlacedBlock,
  PlacedFootnote, PlacedHItem, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow, PositionedBox,
  TableCellBox, TableColumn, TableRowBox, layout_row_cells, max_font_size_in_items, measure_items_width,
};
pub use lowering::{DocumentContent, HeadingRecord, per_page_footnote_numbers};
pub use pipeline::{
  BackMatterInput, BodyLayout, BodyLayoutError, BodyLayoutInput, FrontMatterInput, layout_back_matter, layout_body,
  layout_front_matter,
};

/// 各フィクスチャ（`tests/text/*.sei`）に対して `parse_source → analyze → lowering` を
/// 通し、パニックしないことを確認する統合テスト（旧 `typeset` crate の `tests/smoke.rs`、
/// #307 で本 module 直下の inline テストへ移設）
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::path::PathBuf;

  use super::lowering::{DocumentContent, LayoutNode, LoweringContext, lower_sources_with_headings};
  use crate::{config::Style, frontend::parse_source, model::SourceId};

  /// ワークスペースの `tests/text/<name>.sei` を絶対パスで返す
  fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../tests/text");
    path.push(format!("{name}.sei"));
    return path;
  }

  /// 1 ファイルに対して parse → resolve → lower までを実行し、パニックしないことを確認する
  fn smoke_through_lowering(name: &str) { let _layout_nodes = lower_fixture(name); }

  #[test]
  fn smoke_text_fixture() { smoke_through_lowering("text"); }

  #[test]
  fn smoke_figure_fixture() { smoke_through_lowering("figure"); }

  #[test]
  fn smoke_equation_fixture() { smoke_through_lowering("equation"); }

  #[test]
  fn smoke_align_fixture() { smoke_through_lowering("align"); }

  #[test]
  fn smoke_gather_fixture() { smoke_through_lowering("gather"); }

  #[test]
  fn smoke_split_fixture() { smoke_through_lowering("split"); }

  #[test]
  fn smoke_multiline_fixture() { smoke_through_lowering("multiline"); }

  #[test]
  fn smoke_cases_fixture() { smoke_through_lowering("cases"); }

  #[test]
  fn smoke_matrix_fixture() { smoke_through_lowering("matrix"); }

  #[test]
  fn smoke_ref_fixture() { smoke_through_lowering("ref"); }

  #[test]
  fn smoke_itemize_fixture() { smoke_through_lowering("itemize"); }

  /// `tests/text/<name>.sei` を parse → analyze → lower し、レイアウトノード列を返すヘルパ
  fn lower_fixture(name: &str) -> Vec<LayoutNode> {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
      .unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));
    let style = Style::default();
    let hir = parse_source(&content, SourceId::new(0)).unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));
    let hir_document = crate::model::HirDocument::assemble(vec![hir]);
    let references = crate::citation::References(std::collections::HashMap::new());
    let analyzed =
      crate::resolve::analyze(hir_document, &crate::config::DocumentPolicy::from_style(&style), &references)
        .unwrap_or_else(|e| panic!("analyze 失敗 ({name}): {e:?}"));
    let ctx = LoweringContext::new(&style);
    let citations = crate::citation::GeneratedCitations::default();
    let content = DocumentContent {
      analyzed: &analyzed,
      citations: &citations,
    };
    let (layout_nodes, _headings) = lower_sources_with_headings(&ctx, content);
    return layout_nodes;
  }

  /// レイアウト木を再帰的に辿り、リスト項目の先頭に置かれるマーカー文字列を集める
  fn collect_item_markers(nodes: &[LayoutNode], out: &mut Vec<String>) {
    for node in nodes {
      match node {
        LayoutNode::VBox { children, .. } => {
          if let Some(LayoutNode::Text(text, _)) = children.first() {
            out.push(text.clone());
          }
          collect_item_markers(children, out);
        },
        LayoutNode::HBox { children, .. } => collect_item_markers(children, out),
        _ => {},
      }
    }
  }

  #[test]
  fn itemize_fixture_produces_depth_varying_markers() {
    // Arrange
    let nodes = lower_fixture("itemize");

    // Act
    let mut markers = Vec::new();
    collect_item_markers(&nodes, &mut markers);

    // Assert
    // unordered は • → – → *、ordered は 1. → (a) → i. がそれぞれ生成される。
    for expected in ["• ", "– ", "* ", "1. ", "(a) ", "i. "] {
      assert!(markers.iter().any(|m| return m == expected), "マーカー {expected:?} が見つからない: {markers:?}");
    }
  }

  #[test]
  fn smoke_table_fixture() { smoke_through_lowering("table"); }

  #[test]
  fn smoke_toc_fixture() { smoke_through_lowering("toc"); }

  #[test]
  fn smoke_theorem_fixture() { smoke_through_lowering("theorem"); }
}
