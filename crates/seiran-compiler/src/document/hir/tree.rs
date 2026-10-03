//! HIR の文書単位 [`HirSource`] / [`HirDocument`]。

use crate::document::hir::{HirNode, SourceMap, SourceSpans};

/// 1 ソース分の frontend 出力
///
/// このソースの `SourceId` は位置表 `spans` だけが持つ。
#[derive(Debug, PartialEq)]
pub(crate) struct HirSource {
  /// このソースのトップレベルのブロックノード列
  pub(crate) nodes: Vec<HirNode>,
  /// このソース内の位置表
  pub(crate) spans: SourceSpans,
}

/// プロジェクト全体の authored 文書木
///
/// 著者が書いた内容だけを持ち、書誌・目次・索引のような生成物は含まない。
#[derive(Debug, PartialEq)]
pub(crate) struct HirDocument {
  /// ソースごとのトップレベルのブロックノード列（`SourceId::index()` の昇順）
  source_nodes: Vec<Vec<HirNode>>,
  /// 全ノードのソース位置
  source_map: SourceMap,
}

impl HirDocument {
  /// 全ソースのパース結果から文書木を組み立てる
  ///
  /// `sources` の並び順に依存せず `SourceId::index()` の昇順へ正規化するため、
  /// パースの実行順が `source_nodes` の順序にも `source_map` の内容にも影響しない。
  pub(crate) fn assemble(sources: Vec<HirSource>) -> Self {
    let mut sorted = sources;
    sorted.sort_by_key(|source| return source.spans.source_id().index());

    let mut source_nodes = Vec::with_capacity(sorted.len());
    let mut source_map = SourceMap::default();
    for source in sorted {
      source_map.insert(source.spans);
      source_nodes.push(source.nodes);
    }

    return HirDocument {
      source_nodes,
      source_map,
    };
  }

  /// ソースごとのノード列を返す
  pub(crate) fn source_nodes(&self) -> &[Vec<HirNode>] { return &self.source_nodes; }

  /// 位置表を返す
  pub(crate) fn source_map(&self) -> &SourceMap { return &self.source_map; }
}
