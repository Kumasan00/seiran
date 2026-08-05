//! HIR の文書単位 [`HirSource`] / [`HirGroup`] / [`HirDocument`]。

use crate::model::{
  SourceId,
  hir::{HirNode, SourceMap, SourceSpans},
};

/// 1 ソース分の frontend 出力
#[derive(Debug, PartialEq)]
pub(crate) struct HirSource {
  /// このソースの authored ノード列
  pub(crate) group: HirGroup,
  /// このソース内の位置表
  pub(crate) spans: SourceSpans,
}

/// 1 ソース分の authored ノード列
#[derive(Debug, PartialEq)]
pub(crate) struct HirGroup {
  /// パース元ソースの識別子
  pub(crate) source_id: SourceId,
  /// トップレベルのブロックノード列
  pub(crate) nodes: Vec<HirNode>,
}

/// プロジェクト全体の authored 文書木
///
/// 著者が書いた内容だけを持ち、書誌・目次・索引のような生成物は含まない。
#[derive(Debug, PartialEq)]
pub(crate) struct HirDocument {
  /// ソースごとのノード列（`SourceId::index()` の昇順）
  groups: Vec<HirGroup>,
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
    sorted.sort_by_key(|source| return source.group.source_id.index());

    let mut groups = Vec::with_capacity(sorted.len());
    let mut locations = SourceMap::default();
    for source in sorted {
      locations.insert(source.spans);
      groups.push(source.group);
    }

    return HirDocument { groups, locations };
  }

  /// ソースごとのノード列を返す
  pub(crate) fn groups(&self) -> &[HirGroup] { return &self.groups; }

  /// 位置表を返す
  pub(crate) fn locations(&self) -> &SourceMap { return &self.locations; }
}
