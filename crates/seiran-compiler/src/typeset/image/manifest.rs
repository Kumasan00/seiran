//! 文書木（HIR）が参照する画像パス一覧の収集

use std::collections::BTreeSet;

use crate::{
  document::{HirDocument, HirNode, HirNodeKind},
  project::ProjectPath,
};

/// 文書木（HIR）を再帰的に走査し、画像パスを重複なく収集する（`ProjectPath` の昇順）。
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
      HirNodeKind::Figure(figure) => {
        paths.insert(figure.image_path.clone());
      },
      HirNodeKind::Theorem(theorem) => {
        walk_nodes(&theorem.body, paths);
      },
      HirNodeKind::Quote(quote) => {
        walk_nodes(&quote.body, paths);
      },
      HirNodeKind::List(list) => {
        for item in &list.items {
          walk_nodes(&item.content, paths);
        }
      },
      HirNodeKind::Heading(_)
      | HirNodeKind::CodeBlock(_)
      | HirNodeKind::Paragraph(_)
      | HirNodeKind::MathBlock(_)
      | HirNodeKind::Table(_)
      | HirNodeKind::PageBreak
      | HirNodeKind::Space(_) => {},
    }
  }
}

#[cfg(test)]
mod tests {
  use super::collect_image_paths;
  use crate::{
    document::HirDocument, frontend::test_support::parse_source_for_test, project::ProjectPath, source::SourceId,
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = parse_source_for_test(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// `\image{...}` だけを持つ figure 環境のソースを組み立てる
  fn figure(path: &str) -> String { return format!("\\begin{{figure}}\n\\image{{{path}}}\n\\end{{figure}}\n\n"); }

  #[test]
  fn collects_top_level_figure_paths_deduplicated_and_sorted() {
    let source = format!("{}{}{}", figure("b.png"), figure("a.png"), figure("a.png"));
    let paths = collect_image_paths(&document(&source));
    assert_eq!(paths, vec![ProjectPath::new("a.png"), ProjectPath::new("b.png")]);
  }

  #[test]
  fn collects_paths_that_normalize_to_the_same_file_only_once() {
    let source = format!("{}{}", figure("fig/./a.png"), figure("fig/a.png"));
    let paths = collect_image_paths(&document(&source));

    assert_eq!(paths, vec![ProjectPath::new("fig/a.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_theorem_and_quote_bodies() {
    let source =
      format!("\\begin{{theorem}}\n\\begin{{quote}}\n{}\\end{{quote}}\n\\end{{theorem}}\n", figure("nested.png"));
    let paths = collect_image_paths(&document(&source));
    assert_eq!(paths, vec![ProjectPath::new("nested.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_list_items() {
    let source = format!("\\begin{{itemize}}\n\\item{{{}}}\n\\end{{itemize}}\n", figure("in-list.png").trim_end());
    let paths = collect_image_paths(&document(&source));
    assert_eq!(paths, vec![ProjectPath::new("in-list.png")]);
  }

  #[test]
  fn returns_empty_manifest_when_no_figures_present() {
    let source = "\\pagebreak\n";
    let paths = collect_image_paths(&document(source));
    assert_eq!(paths, []);
  }
}
