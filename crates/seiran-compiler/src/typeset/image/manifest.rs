//! 文書木（HIR）が参照する画像パス一覧の収集

use std::collections::BTreeSet;

use crate::{
  document::{HirDocument, HirNode, HirNodeKind},
  project::ProjectPath,
};

/// 文書木（HIR）を再帰的に走査し、画像パスを重複なく収集する（`ProjectPath` の昇順）。
///
/// 定理、引用、リスト内の入れ子も探索する。
pub(crate) fn collect_image_paths(document: &HirDocument) -> Vec<ProjectPath> {
  let mut paths: BTreeSet<ProjectPath> = BTreeSet::new();
  for group in document.groups() {
    walk_nodes(&group.nodes, &mut paths);
  }
  return paths.into_iter().collect();
}

/// `nodes` を再帰的に走査し、`Figure` の `image_path` を `paths` へ集める。
fn walk_nodes(nodes: &[HirNode], paths: &mut BTreeSet<ProjectPath>) {
  for node in nodes {
    match &node.kind {
      HirNodeKind::Figure { image_path, .. } => {
        paths.insert(image_path.clone());
      },
      HirNodeKind::Theorem { body, .. } | HirNodeKind::Quote { body, .. } => {
        walk_nodes(body, paths);
      },
      HirNodeKind::List { items, .. } => {
        for item in items {
          walk_nodes(&item.content, paths);
        }
      },
      HirNodeKind::Heading { .. }
      | HirNodeKind::Paragraph(_)
      | HirNodeKind::MathBlock { .. }
      | HirNodeKind::Table { .. }
      | HirNodeKind::Rule { .. }
      | HirNodeKind::PageBreak
      | HirNodeKind::Space(_) => {},
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::collect_image_paths;
  use crate::{document::HirDocument, project::ProjectPath, source::SourceId};

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// `\image{...}` だけを持つ figure 環境のソースを組み立てる
  fn figure(path: &str) -> String { return format!("\\begin{{figure}}\n\\image{{{path}}}\n\\end{{figure}}\n\n"); }

  #[test]
  fn collects_top_level_figure_paths_deduplicated_and_sorted() {
    // Arrange
    let source = format!("{}{}{}", figure("b.png"), figure("a.png"), figure("a.png"));

    // Act
    let paths = collect_image_paths(&document(&source));

    // Assert — 重複が除かれ、昇順で並ぶ
    assert_eq!(paths, vec![ProjectPath::new("a.png"), ProjectPath::new("b.png")]);
  }

  #[test]
  fn collects_paths_that_normalize_to_the_same_file_only_once() {
    // Arrange — 同じファイルを冗長な `.` 付きと素の形で 2 回参照する
    let source = format!("{}{}", figure("fig/./a.png"), figure("fig/a.png"));

    // Act
    let paths = collect_image_paths(&document(&source));

    // Assert — `ProjectPath` の正規化は重複除去より前に効くので 1 件に畳まれる
    // （画像パスの型が `ProjectPath` へ一本化された結果。同じファイルを 2 回読まない）
    assert_eq!(paths, vec![ProjectPath::new("fig/a.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_theorem_and_quote_bodies() {
    // Arrange — Quote の中に Figure、その Quote を Theorem の body に入れて 2 段ネストさせる
    let source =
      format!("\\begin{{theorem}}\n\\begin{{quote}}\n{}\\end{{quote}}\n\\end{{theorem}}\n", figure("nested.png"));

    // Act
    let paths = collect_image_paths(&document(&source));

    // Assert
    assert_eq!(paths, vec![ProjectPath::new("nested.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_list_items() {
    // Arrange — \item{...} の中に figure 環境が入るケース
    let source = format!("\\begin{{itemize}}\n\\item{{{}}}\n\\end{{itemize}}\n", figure("in-list.png").trim_end());

    // Act
    let paths = collect_image_paths(&document(&source));

    // Assert
    assert_eq!(paths, vec![ProjectPath::new("in-list.png")]);
  }

  #[test]
  fn returns_empty_manifest_when_no_figures_present() {
    // Arrange
    let source = "\\pagebreak\n";

    // Act
    let paths = collect_image_paths(&document(source));

    // Assert
    assert!(paths.is_empty());
  }
}
