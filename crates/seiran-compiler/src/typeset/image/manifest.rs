//! 文書木（HIR）が参照する画像の収集

use std::collections::{BTreeMap, BTreeSet};

use crate::{
  document::{HirDocument, HirNode, HirNodeKind, ImageFormat},
  project::ProjectPath,
};

/// 文書が参照する画像 1 ファイルぶんの要求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::typeset) struct ImageRequest {
  /// 画像ファイルへのパス（正規化済み）
  pub(in crate::typeset) path: ProjectPath,
  /// 拡張子が宣言する画像形式（同じパスは同じ拡張子なので 1 つに決まる）
  pub(in crate::typeset) format: ImageFormat,
  /// ソースに書かれた `page` の集合（省略は `None`。PDF 以外は frontend が `page` を拒否するので `{None}`）
  pub(in crate::typeset) pages: BTreeSet<Option<u32>>,
}

/// 文書木（HIR）を再帰的に走査し、画像ファイルごとの要求を重複なく収集する（`ProjectPath` の昇順）。
pub(in crate::typeset) fn collect_image_requests(document: &HirDocument) -> Vec<ImageRequest> {
  let mut requests: BTreeMap<ProjectPath, ImageRequest> = BTreeMap::new();
  for nodes in document.source_nodes() {
    walk_nodes(nodes, &mut requests);
  }
  return requests.into_values().collect();
}

/// `nodes` を再帰的に走査し、`Figure` のパス・形式・`page` を `requests` へ集める。
fn walk_nodes(nodes: &[HirNode], requests: &mut BTreeMap<ProjectPath, ImageRequest>) {
  for node in nodes {
    match &node.kind {
      HirNodeKind::Figure(figure) => {
        let request = requests.entry(figure.image_path.clone()).or_insert_with(|| {
          return ImageRequest {
            path: figure.image_path.clone(),
            format: figure.format,
            pages: BTreeSet::new(),
          };
        });
        request.pages.insert(figure.page);
      },
      HirNodeKind::Theorem(theorem) => {
        walk_nodes(&theorem.body, requests);
      },
      HirNodeKind::Quote(quote) => {
        walk_nodes(&quote.body, requests);
      },
      HirNodeKind::Flush(flush) => {
        walk_nodes(&flush.body, requests);
      },
      HirNodeKind::List(list) => {
        for item in &list.items {
          walk_nodes(&item.body, requests);
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
  use super::collect_image_requests;
  use crate::{
    document::{HirDocument, ImageFormat},
    frontend::test_support::parse_for_test,
    project::ProjectPath,
    source::SourceId,
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = parse_for_test(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// ソース 1 本が参照する画像のパスを収集順に返す
  fn collected_paths(source: &str) -> Vec<ProjectPath> {
    return collect_image_requests(&document(source)).into_iter().map(|request| return request.path).collect();
  }

  /// `\image{...}` だけを持つ figure 環境のソースを組み立てる
  fn figure(path: &str) -> String { return format!("\\begin{{figure}}\n\\image{{{path}}}\n\\end{{figure}}\n\n"); }

  #[test]
  fn collects_top_level_figure_paths_deduplicated_and_sorted() {
    let source = format!("{}{}{}", figure("b.png"), figure("a.png"), figure("a.png"));
    let paths = collected_paths(&source);
    assert_eq!(paths, vec![ProjectPath::new("a.png"), ProjectPath::new("b.png")]);
  }

  #[test]
  fn collects_paths_that_normalize_to_the_same_file_only_once() {
    let source = format!("{}{}", figure("fig/./a.png"), figure("fig/a.png"));
    let paths = collected_paths(&source);

    assert_eq!(paths, vec![ProjectPath::new("fig/a.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_theorem_and_quote_bodies() {
    let source =
      format!("\\begin{{theorem}}\n\\begin{{quote}}\n{}\\end{{quote}}\n\\end{{theorem}}\n", figure("nested.png"));
    let paths = collected_paths(&source);
    assert_eq!(paths, vec![ProjectPath::new("nested.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_flush_body() {
    let source = format!("\\begin{{flushright}}\n{}\\end{{flushright}}\n", figure("flush.png"));
    let paths = collected_paths(&source);
    assert_eq!(paths, vec![ProjectPath::new("flush.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_list_items() {
    let source = format!("\\begin{{itemize}}\n\\item{{{}}}\n\\end{{itemize}}\n", figure("in-list.png").trim_end());
    let paths = collected_paths(&source);
    assert_eq!(paths, vec![ProjectPath::new("in-list.png")]);
  }

  #[test]
  fn collects_written_pages_per_pdf_file() {
    let source =
      "\\begin{figure}\n\\image[page=2]{a.pdf}\n\\end{figure}\n\n\\begin{figure}\n\\image{a.pdf}\n\\end{figure}\n\n";
    let requests = collect_image_requests(&document(source));

    assert_eq!(requests.len(), 1, "同じ PDF は 1 ファイルの要求にまとまるはず: {requests:?}");
    assert_eq!(requests[0].format, ImageFormat::Pdf);
    assert_eq!(requests[0].pages.iter().copied().collect::<Vec<_>>(), vec![None, Some(2)]);
  }

  #[test]
  fn returns_empty_manifest_when_no_figures_present() {
    let source = "\\pagebreak\n";
    let paths = collected_paths(source);
    assert_eq!(paths, []);
  }
}
