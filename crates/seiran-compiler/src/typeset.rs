//! 組版 module — 意味解析の成果物（`semantics::SemanticDocument`）を、描画直前の確定レイアウト
//! （[`LaidOutDocument`]）へ変換する（旧 `typeset` crate、#307 で `seiran` の非公開 module として吸収）
//!
//! 外向きの入口は [`layout`] の 1 操作だけで、段順序（lowering → 計測 → 画像寸法確定 → 行分割・
//! 改ページ → 前付け・後付け → ページラベル → 走り文 → outline）と、その間に成立する不変条件
//! （box 計測は 1 回だけ・`breaking` はフォントに触れない）はすべて実装側に閉じる（#350）。
//!
//! 組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）は本 module 非公開の
//! 子 module `boxes` が所有する（#280、#350 で `layout` から改名）。`compiler::publication` が
//! `Publication` へ写すために読むぶんだけを facade へ出す。

mod block;
mod boxes;
mod breaking;
mod error;
mod image;
mod lowering;
mod pagination;

// 確定レイアウトを `Publication` へ写す `compiler::publication` が、ページの中身（配置済みブロック・
// 表の行・箱の内容）を走査するために名指しする型と、表セルの配置・計測ヘルパ。`HBox` / `Line` /
// `Placed*` / `PositionedBox` / `TableCellBox` / `TableRowBox` / `measure_items_width` は
// `compiler` 配下の `#[cfg(test)] mod tests`（`dump` / `publication` のテスト）が組版済みページを
// 組み立て・走査するために要る。組版の段を呼ぶための型（`PageGeometry` / `KnuthPlassBreaker` /
// 各段の入力）は入口が `layout` 1 操作になったので facade から外した（#350）。同様に
// `Align` / `FootnoteId` も `typeset` の外に消費者がいない（#326）。
#[allow(unused_imports)]
pub(crate) use boxes::{
  AnchorId, AnchorMark, HBox, HBoxContent, HItem, Line, LinkTarget, Page, PlacedAnchor, PlacedBlock, PlacedFootnote,
  PlacedHItem, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow, PositionedBox, TableCellBox,
  TableColumn, TableRowBox, layout_row_cells, max_font_size_in_items, measure_items_width,
};
pub(crate) use error::TypesetError;
pub(crate) use pagination::LaidOutDocument;
// `OutlineEntry` を本体コードから名指しする消費者はいない（`compiler::publication` は
// `laid_out.outline_entries` をフィールドとして走査するだけ）。`publication` の
// `#[cfg(test)] mod tests` が確定レイアウトを組み立てるために必要なので facade へ出す。
#[allow(unused_imports)]
pub(crate) use pagination::OutlineEntry;

use crate::{font::FontSystem, semantics::SemanticDocument};

/// 意味解析の成果物を、描画直前の確定レイアウトへ組版する。
///
/// 画像は `document` が参照しているぶんだけを `source` 経由で読み込み、自然寸法から表示寸法を
/// 確定して結果へ同梱する（生バイト列は描画の資源束が要求する）。フォント資源は呼び出し元が
/// 構築したものを借りる（`font` module の移設は別 issue）。
///
/// # Errors
///
/// 画像の読込・デコード・寸法確定、または脚注のページ単位採番の収束に失敗した場合にエラーを返す。
pub(crate) fn layout(
  source: &dyn crate::project::ProjectSource,
  config: &crate::config::Config,
  style: &crate::style::Style,
  font_system: &FontSystem<'_>,
  document: &SemanticDocument,
) -> Result<LaidOutDocument, TypesetError> {
  let image_paths = image::collect_image_paths(document.hir());
  let images = image::load_image_resources(source, &image_paths)?;
  let ctx = pagination::TypesetContext::new(config, style, font_system);
  return pagination::paginate(&ctx, document, images, image_paths);
}

/// 各フィクスチャ（`tests/text/*.sei`）に対して `parse_source → analyze → lowering` を
/// 通し、パニックしないことを確認する統合テスト（旧 `typeset` crate の `tests/smoke.rs`、
/// #307 で本 module 直下の inline テストへ移設）
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::path::PathBuf;

  use super::lowering::{LayoutNode, LoweringContext, lower_sources_with_headings};
  use crate::{frontend::parse_source, source::SourceId, style::Style};

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
    let hir_document = crate::document::HirDocument::assemble(vec![hir]);
    let references = crate::semantics::References(std::collections::HashMap::new());
    let analyzed =
      crate::semantics::analyze_for_test(hir_document, &crate::config::DocumentPolicy::from_style(&style), &references)
        .unwrap_or_else(|e| panic!("analyze 失敗 ({name}): {e:?}"));
    let ctx = LoweringContext::new(&style);
    let (layout_nodes, _headings) = lower_sources_with_headings(&ctx, &analyzed);
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
