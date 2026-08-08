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
//!
//! フォント処理（OpenType 解析・検証・メトリクス・シェイピング）は子 module `font` が持つ
//! （#352）。入力の 19 種別・設定・バイト列は `project::font` の所有で、この module はそこから
//! フォント資源を組み立てて使う側になる。

mod block;
mod boxes;
mod breaking;
mod error;
mod font;
mod geometry;
mod image;
mod lowering;
mod pagination;

// 確定ページ列の決定的テキストダンプ（golden 比較用）。走査対象が `boxes` の中間型なので所有は
// こちら側で、`compiler::golden` は `dump_pages` の 1 関数だけを借りる（#353）。
#[cfg(test)]
mod dump;
// 外側の module のテストが確定レイアウトを組み立てるための fixture builder（#353）。
#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod test_fixtures;

// 確定レイアウトを `Publication` へ写す `compiler::publication` が、ページの中身（配置済みブロック・
// 表の行・箱の内容）を走査するために名指しする型と、表セルの配置・計測ヘルパ。この `boxes` からの
// 再エクスポートに載るのは **本体コードに消費者がある名前だけ**で、テストが確定レイアウトを
// 組み立てる手段は `#[cfg(test)]` の子 module `test_fixtures` が持つ（#353）。組版の段を呼ぶための型
// （`PageGeometry` / `KnuthPlassBreaker` / 各段の入力）は入口が `layout` 1 操作になったので
// facade から外した（#350）。同様に `Align` / `FootnoteId` も `typeset` の外に消費者がいない（#326）。
pub(crate) use boxes::{
  AnchorId, AnchorMark, HBoxContent, HItem, LinkTarget, Page, PlacedBlock, PlacedTableRow, TableColumn,
  layout_row_cells, max_font_size_in_items,
};
// テスト専用の例外 — `compiler::golden` / `compiler::project_source_equivalence` が確定ページ列を
// ダンプ比較するための関数 1 つだけを出す（中間型そのものは出さない）。
#[cfg(test)]
pub(crate) use dump::dump_pages;
pub(crate) use error::TypesetError;
// フォント資源のハンドル `FontResources` と、`compiler::publication` が `Publication` へ写す
// シェイピング結果。フォントの解析・検証・シェーパー構築は `font` に閉じており、`FontSystem` /
// `FontRefs` / `FontMetrics` / 拡張 trait は `typeset` の外から見えない（#352）。
pub(crate) use font::{FontResources, Glyph, GlyphRun};
// 入口は `layout` 1 操作という原則の意図した例外（#351）。用紙・余白 × 段組みの横断制約は
// 組版の不変条件なのでここが所有するが、**呼び出しは入力読込（`compiler::input::load`）の中**で
// 行う — 不正な組み合わせを組版より前に弾き、診断の出るタイミングを変えないため。
pub(crate) use geometry::{LayoutValidationError, validate_layout};
pub(crate) use pagination::LaidOutDocument;

use crate::{project::config::ProjectConfig, semantics::SemanticDocument, style::Style};

/// 意味解析の成果物を、描画直前の確定レイアウトへ組版する。
///
/// 画像は `document` が参照しているぶんだけを `source` 経由で読み込み、自然寸法から表示寸法を
/// 確定して結果へ同梱する（生バイト列は描画の資源束が要求する）。フォント資源は呼び出し元が
/// 構築したものを借り、そこからシェーパーを組むのはこの中（`font` module に閉じる、#352）。
///
/// # Errors
///
/// シェーパーの構築、画像の読込・デコード・寸法確定、または脚注のページ単位採番の収束に
/// 失敗した場合にエラーを返す。
pub(crate) fn layout(
  source: &dyn crate::project::ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  font_resources: &FontResources<'_>,
  document: &SemanticDocument,
) -> Result<LaidOutDocument, TypesetError> {
  // シェーパー構築は画像読込より前に置く — 両方が失敗する入力で報告されるエラーを、
  // フォント資源を呼び出し元が組んでいた頃と同じ側（フォント）に保つため。
  let font_system = font_resources.system()?;
  let image_paths = image::collect_image_paths(document.hir());
  let images = image::load_image_resources(source, &image_paths)?;
  let ctx = pagination::TypesetContext::new(config, style, &font_system);
  return pagination::paginate(&ctx, document, images, image_paths);
}

/// lowering の単体テスト
///
/// かつてここには各フィクスチャを `parse_source → analyze → lowering` に通して
/// パニックしないことだけを見る `smoke_*_fixture` が 14 件あったが、対象のフィクスチャは
/// すべて上位のテスト面（`figure` は `compiler::pdf_structure` の `PDF_STRUCTURE_INPUTS`、
/// 残る 13 件は `compiler::golden` の `GOLDEN_INPUTS`）が確定出力ごと assert しているため
/// 削除した（#353）。ここに残すのは lowering の結果を実際に検証するものだけ。
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

  /// `tests/text/<name>.sei` を parse → analyze → lower し、レイアウトノード列を返すヘルパ
  fn lower_fixture(name: &str) -> Vec<LayoutNode> {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
      .unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));
    let style = Style::default();
    let hir = parse_source(&content, SourceId::new(0)).unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));
    let hir_document = crate::document::HirDocument::assemble(vec![hir]);
    let references = crate::semantics::References(std::collections::HashMap::new());
    let analyzed = crate::semantics::analyze_for_test(
      hir_document,
      &crate::semantics::SemanticPolicy::from_style(&style),
      &references,
    )
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
}
