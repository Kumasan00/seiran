//! `parse_project` が返す画像パス一覧（`ImageManifest`）の収集

use std::collections::BTreeSet;

use model::{AssetId, DocNode};

/// 画像パスの一覧（重複なし・パス文字列の昇順）。
pub(super) struct ImageManifest {
  /// 画像ファイルへのパス（`\image{...}` の必須引数、重複なし・昇順）
  pub(super) paths: Vec<AssetId>,
}

/// 全 `DocNode` を再帰的に走査し、画像パスを重複なく収集する。
///
/// 定理、引用、リスト内の入れ子も探索する。
pub(super) fn collect_image_paths(groups: &[&[DocNode]]) -> ImageManifest {
  let mut paths: BTreeSet<AssetId> = BTreeSet::new();
  for group in groups {
    walk_nodes(group, &mut paths);
  }
  return ImageManifest {
    paths: paths.into_iter().collect(),
  };
}

/// `nodes` を再帰的に走査し、`Figure` の `image_path` を `paths` へ集める。
fn walk_nodes(nodes: &[DocNode], paths: &mut BTreeSet<AssetId>) {
  for node in nodes {
    match node {
      DocNode::Figure { image_path, .. } => {
        paths.insert(image_path.clone());
      },
      DocNode::Theorem { body, .. } | DocNode::Quote { body, .. } => {
        walk_nodes(body, paths);
      },
      DocNode::List { items, .. } => {
        for item in items {
          walk_nodes(&item.content, paths);
        }
      },
      DocNode::Heading { .. }
      | DocNode::Paragraph(_)
      | DocNode::MathBlock { .. }
      | DocNode::Table { .. }
      | DocNode::Rule { .. }
      | DocNode::PageBreak
      | DocNode::Space(_)
      | DocNode::Anchor(_) => {},
    }
  }
}

#[cfg(test)]
mod tests {
  use model::{AssetId, CaptionPosition, DocNode, ListItem, QuoteKind, Span, TheoremClass};

  use super::collect_image_paths;

  /// `image_path` だけを差し替えた最小の `Figure` ノードを作るテストヘルパ
  fn figure(path: &str) -> DocNode {
    return DocNode::Figure {
      image_path: AssetId::new(path),
      width: None,
      height: None,
      dpi: None,
      downsample: None,
      caption: None,
      caption_position: CaptionPosition::Bottom,
      label: None,
      span: Span::DUMMY,
    };
  }

  #[test]
  fn collects_top_level_figure_paths_deduplicated_and_sorted() {
    // Arrange
    let group = vec![figure("b.png"), figure("a.png"), figure("a.png")];

    // Act
    let manifest = collect_image_paths(&[group.as_slice()]);

    // Assert — 重複が除かれ、パス文字列の昇順で並ぶ
    assert_eq!(manifest.paths, vec![AssetId::new("a.png"), AssetId::new("b.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_theorem_and_quote_bodies() {
    // Arrange — Quote の中に Figure、その Quote を Theorem の body に入れて 2 段ネストさせる
    let quote = DocNode::Quote {
      kind: QuoteKind::Quote,
      body: vec![figure("nested.png")],
    };
    let theorem = DocNode::Theorem {
      class: TheoremClass::Theorem,
      title: None,
      body: vec![quote],
      of: None,
      label: None,
      span: Span::DUMMY,
    };

    // Act
    let manifest = collect_image_paths(&[&[theorem]]);

    // Assert
    assert_eq!(manifest.paths, vec![AssetId::new("nested.png")]);
  }

  #[test]
  fn collects_figure_paths_nested_in_list_items() {
    // Arrange — \item{...} の中に figure 環境が入るケース
    let item = ListItem::new(vec![figure("in-list.png")]);
    let list = DocNode::List {
      ordered: false,
      items: vec![item],
      start: None,
      item_gap: None,
    };

    // Act
    let manifest = collect_image_paths(&[&[list]]);

    // Assert
    assert_eq!(manifest.paths, vec![AssetId::new("in-list.png")]);
  }

  #[test]
  fn returns_empty_manifest_when_no_figures_present() {
    // Arrange
    let group = vec![DocNode::PageBreak];

    // Act
    let manifest = collect_image_paths(&[group.as_slice()]);

    // Assert
    assert!(manifest.paths.is_empty());
  }
}
