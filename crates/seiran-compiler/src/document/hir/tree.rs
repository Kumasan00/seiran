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
  groups: Vec<Vec<HirNode>>,
  /// 全ノードのソース位置
  locations: SourceMap,
}

impl HirDocument {
  /// 全ソースのパース結果から文書木を組み立てる
  ///
  /// `sources` の並び順に依存せず `SourceId::index()` の昇順へ正規化するため、
  /// パースの実行順が `groups` の順序にも `SourceMap` の内容にも影響しない。
  pub(crate) fn assemble(sources: Vec<HirSource>) -> Self {
    let mut sorted = sources;
    sorted.sort_by_key(|source| return source.spans.source_id().index());

    let mut groups = Vec::with_capacity(sorted.len());
    let mut locations = SourceMap::default();
    for source in sorted {
      locations.insert(source.spans);
      groups.push(source.nodes);
    }

    return HirDocument { groups, locations };
  }

  /// ソースごとのノード列を返す
  pub(crate) fn groups(&self) -> &[Vec<HirNode>] { return &self.groups; }

  /// 位置表を返す
  pub(crate) fn locations(&self) -> &SourceMap { return &self.locations; }
}
