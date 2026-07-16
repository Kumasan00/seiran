//! 各フィクスチャ（`tests/text/*.sei`）に対して `parse_source → lower_nodes` を
//! パニックなしで通す smoke テスト
//!
//! フォント読み込みを避けるため `lower_nodes` までで打ち切り、出力構造は検証しない。
//! `lower_nodes` より下（`build_blocks` / `break_pages`）の検証は各クレート側に委ねる。

use std::{collections::HashSet, path::PathBuf};

use config::Style;
use frontend::parse_source;
use typeset::{LayoutNode, LoweringContext};

/// ワークスペースの `tests/text/<name>.sei` を絶対パスで返す
fn fixture_path(name: &str) -> PathBuf {
  let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  path.push("../../tests/text");
  path.push(format!("{name}.sei"));
  return path;
}

/// 1 ファイルに対して parse → lower までを実行し、パニックしないことを確認する
fn smoke_through_lowering(name: &str) {
  let path = fixture_path(name);
  let content =
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));

  let style = Style::default();
  let doc_nodes = parse_source(&content, &path.display().to_string(), &HashSet::new())
    .unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));

  let ctx = LoweringContext::new(&style);
  let _layout_nodes =
    typeset::lower_nodes(&ctx, &doc_nodes).unwrap_or_else(|e| panic!("lower_nodes 失敗 ({name}): {e:?}"));
}

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

/// `tests/text/<name>.sei` を parse → lower し、レイアウトノード列を返すヘルパ
fn lower_fixture(name: &str) -> Vec<LayoutNode> {
  let path = fixture_path(name);
  let content =
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));
  let style = Style::default();
  let doc_nodes = parse_source(&content, &path.display().to_string(), &HashSet::new())
    .unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));
  let ctx = LoweringContext::new(&style);
  return typeset::lower_nodes(&ctx, &doc_nodes).unwrap_or_else(|e| panic!("lower_nodes 失敗 ({name}): {e:?}"));
}

/// レイアウト木を再帰的に辿り、リスト項目の先頭に置かれるマーカー文字列を集める
///
/// `lower_list` は各項目 `VBox` の先頭に `Text(marker, _)` を置く。ここでは
/// 「先頭の子が `Text` である `VBox`」の先頭文字列を収集し、深さ別マーカーの検証に使う。
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
  // Arrange — ネストを含む itemize フィクスチャを parse → lower する
  let nodes = lower_fixture("itemize");

  // Act — レイアウト木からリスト項目のマーカー文字列を集める
  let mut markers = Vec::new();
  collect_item_markers(&nodes, &mut markers);

  // Assert — parse → lower を通した全パイプラインで、深さ別マーカーが実際に現れる。
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
