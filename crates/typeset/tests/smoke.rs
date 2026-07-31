//! 各フィクスチャ（`tests/text/*.sei`）に対して `parse_source → resolve_project → lowering` を

use std::{collections::HashSet, path::PathBuf};

use config::Style;
use frontend::parse_source;
use model::{Origin, SourceId};
use resolve::{SemanticDocument, SemanticGroup};
use typeset::{LayoutNode, LoweringContext};

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
  let content =
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));
  let style = Style::default();
  let doc_nodes = parse_source(&content, &path.display().to_string(), &HashSet::new())
    .unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));
  let semantic = SemanticDocument {
    groups: vec![SemanticGroup {
      nodes: &doc_nodes,
      origin: Origin::Source(SourceId::new(0)),
    }],
    bibliography: &[],
  };
  let document =
    resolve::resolve_project(&semantic, &style).unwrap_or_else(|e| panic!("resolve_project 失敗 ({name}): {e:?}"));
  let ctx = LoweringContext::new(&style);
  let (layout_nodes, _headings) = typeset::lower_sources_with_headings(&ctx, &document);
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
