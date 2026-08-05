//! 組版パス統合 module — 解決済みドキュメント（`resolve::ResolvedDocument`）から計測済み・
//! 配置済みページ直前までを担う（旧 `typeset` crate、#307 で `seiran` の非公開 module として吸収）
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
// `GreedyBreaker` / `LineBreaker` は旧 typeset crate の公開 API 保持のための再エクスポートで、
// 現状 seiran 内には crate::typeset root 経由の利用者がいない（LineBreaker は break_pages 内部が
// `super::break_lines::LineBreaker` で直接参照、GreedyBreaker は breaking 配下のテストが個別 import
// する）。API 面は維持しつつ、この再エクスポート自体の unused_imports のみ抑制する。
#[allow(unused_imports)]
pub use breaking::{GreedyBreaker, KnuthPlassBreaker, LineBreaker, PageGeometry};
// `CellPlacement` / `LineFootnote` / `LineIndexEntry` / `LineLink` / `MathRowNumber` / `TableBox` は
// 同様に旧 typeset crate の公開 API 保持のための再エクスポートで、crate::typeset root 経由の
// 利用者は現状なし（内部の他 module からは個別 import されている）。
#[allow(unused_imports)]
pub use layout::{
  Block, CellPlacement, HBox, HBoxContent, HItem, Line, LineFootnote, LineIndexEntry, LineLink, MathRowNumber, Page,
  PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedHItem, PlacedIndexEntry, PlacedLink, PlacedMathNumber,
  PlacedTableRow, PositionedBox, TableBox, TableCellBox, TableRowBox, layout_row_cells, max_font_size_in_items,
  measure_items_width,
};
// `LayoutNode` / `LoweringContext` / `MathBlockRow` / `TableCellLayout` / `TableLayout` /
// `TableRowLayout` / `TextStyle` / `lower_sources_with_headings` は本 module 直下の
// `#[cfg(test)] mod tests`（旧 typeset crate の統合スモークテスト）でのみ使われ、`cfg(test)` を
// 有効化しない通常ビルドでは未使用に見える。API 面の維持のため再エクスポート自体は残す。
#[allow(unused_imports)]
pub use lowering::{
  HeadingRecord, LayoutNode, LoweringContext, MathBlockRow, TableCellLayout, TableLayout, TableRowLayout, TextStyle,
  lower_sources_with_headings, per_page_footnote_numbers,
};
pub use pipeline::{
  BackMatterInput, BodyLayout, BodyLayoutError, BodyLayoutInput, FrontMatterInput, layout_back_matter, layout_body,
  layout_front_matter,
};

/// 各フィクスチャ（`tests/text/*.sei`）に対して `parse_source → resolve_project → lowering` を
/// 通し、パニックしないことを確認する統合テスト（旧 `typeset` crate の `tests/smoke.rs`、
/// #307 で本 module 直下の inline テストへ移設）
///
/// `frontend::parse_source` は #307 Task 6 で `crate::frontend` へ吸収済みのため
/// `crate::frontend::` 参照に更新済み。`resolve::resolve_project` は #307 Task 5 で
/// `crate::resolve` へ吸収済みのため `crate::resolve::` 参照に更新済み。
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::{collections::HashSet, path::PathBuf};

  use super::{LayoutNode, LoweringContext, lower_sources_with_headings};
  use crate::{
    config::Style,
    frontend::parse_source,
    model::SourceId,
    resolve::{SemanticDocument, SemanticGroup},
  };

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

  /// `tests/text/<name>.sei` を parse → resolve → lower し、レイアウトノード列を返すヘルパ
  fn lower_fixture(name: &str) -> Vec<LayoutNode> {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
      .unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));
    let style = Style::default();
    let hir = parse_source(&content, SourceId::new(0), &HashSet::new())
      .unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));
    let hir_document = crate::model::HirDocument::assemble(vec![hir]);
    let group = hir_document.groups().first().expect("1 ソース分のグループがあるはず");
    let doc_nodes = crate::frontend::hir_group_to_doc_nodes(group, hir_document.locations());
    let semantic = SemanticDocument {
      groups: vec![SemanticGroup {
        nodes: &doc_nodes,
        source_id: SourceId::new(0),
      }],
      bibliography: &[],
    };
    let document = crate::resolve::resolve_project(&semantic, &style)
      .unwrap_or_else(|e| panic!("resolve_project 失敗 ({name}): {e:?}"));
    let ctx = LoweringContext::new(&style);
    let (layout_nodes, _headings) = lower_sources_with_headings(&ctx, &document);
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
