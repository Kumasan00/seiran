//! HIR ノードのソース位置表 [`SourceSpans`] / [`SourceMap`]。

use crate::{
  document::hir::NodeId,
  source::{SourceId, Span},
};

/// 1 ソース分の位置表（添字 = `NodeId::local`）
///
/// `alloc` が ID 発行と位置記録を同時に行うため、発行済みの `NodeId` は必ず位置を引ける。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceSpans {
  /// この表が属するソース
  source_id: SourceId,
  /// `local` 順に並んだ span
  spans: Vec<Span>,
}

impl SourceSpans {
  /// 空の位置表を作る
  pub(super) fn new(source_id: SourceId) -> Self {
    return SourceSpans {
      source_id,
      spans: Vec::new(),
    };
  }

  /// 新しい `NodeId` を発行し、同時に span を記録する
  ///
  /// 親ノードの ID を子より先に確保する（予約する）用途にも使う。
  pub(super) fn alloc(&mut self, span: Span) -> NodeId {
    let Ok(local) = u32::try_from(self.spans.len()) else {
      unreachable!("1 ソースの HIR ノード数が u32::MAX を超えることはない")
    };
    self.spans.push(span);
    return NodeId::new(self.source_id, local);
  }

  /// 予約済み ID の span を確定させる（段落のように閉じ位置が後から決まるノード用）
  pub(super) fn set_span(&mut self, id: NodeId, span: Span) {
    let Some(slot) = self.spans.get_mut(id.local() as usize) else {
      unreachable!("set_span の対象 ID は同じ SourceSpans が alloc したものである")
    };
    *slot = span;
    return;
  }

  /// 発行済み ID の span を返す
  pub(crate) fn span_of(&self, id: NodeId) -> Span {
    let Some(span) = self.spans.get(id.local() as usize) else {
      unreachable!("span_of の対象 ID は同じ SourceSpans が alloc したものである")
    };
    return *span;
  }

  /// 属するソースを返す
  pub(crate) fn source_id(&self) -> SourceId { return self.source_id; }

  /// 発行済み ID 数を返す
  #[allow(dead_code, reason = "crate 内の `#[cfg(test)]` からのみ使う")]
  pub(crate) fn len(&self) -> usize { return self.spans.len(); }
}

/// 全ソース分をまとめた位置表（添字 = `SourceId::index()`）
///
/// 挿入順ではなく `SourceId` の index 位置へ差し込むため、パースの実行順が内容へ影響しない。
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceMap {
  /// ソース ID の index 位置に各ソースの位置表を置く。未登録の位置は `None`
  per_source: Vec<Option<SourceSpans>>,
}

impl SourceMap {
  /// 1 ソース分の位置表を `source_id.index()` の位置へ差し込む
  pub(super) fn insert(&mut self, spans: SourceSpans) {
    let index = spans.source_id().index();
    if self.per_source.len() <= index {
      self.per_source.resize_with(index + 1, || return None);
    }
    self.per_source[index] = Some(spans);
    return;
  }

  /// `NodeId` から位置を引く
  pub(crate) fn get(&self, id: NodeId) -> Option<SourceLocation> {
    let spans = self.per_source.get(id.source().index())?.as_ref()?;
    let span = *spans.spans.get(id.local() as usize)?;
    return Some(SourceLocation {
      source_id: spans.source_id(),
      span,
    });
  }

  /// `NodeId` から位置を引く（引けない場合は不変条件の破れとして落とす）
  pub(crate) fn location(&self, id: NodeId) -> SourceLocation {
    let Some(location) = self.get(id) else {
      unreachable!("NodeId は同じ HirDocument の HirBuilder が発行したもので、発行と同時に span が記録される")
    };
    return location;
  }
}

/// HIR ノード 1 個のソース位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceLocation {
  /// 位置が属するソース
  pub(crate) source_id: SourceId,
  /// ソース内のバイト範囲
  pub(crate) span: Span,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn alloc_records_span_and_increments_local() {
    // Arrange
    let mut spans = SourceSpans::new(SourceId::new(0));

    // Act
    let first = spans.alloc(Span::new(0, 3));
    let second = spans.alloc(Span::new(3, 7));

    // Assert
    assert_eq!(first.local(), 0);
    assert_eq!(second.local(), 1);
    assert_eq!(spans.span_of(first), Span::new(0, 3));
    assert_eq!(spans.span_of(second), Span::new(3, 7));
  }

  #[test]
  fn set_span_overwrites_reserved_span() {
    // Arrange
    let mut spans = SourceSpans::new(SourceId::new(0));
    let id = spans.alloc(Span::new(5, 5));

    // Act
    spans.set_span(id, Span::new(5, 12));

    // Assert
    assert_eq!(spans.span_of(id), Span::new(5, 12));
  }

  #[test]
  fn source_map_is_indexed_by_source_id_not_insertion_order() {
    // Arrange
    let mut first = SourceSpans::new(SourceId::new(0));
    let a = first.alloc(Span::new(0, 1));
    let mut second = SourceSpans::new(SourceId::new(1));
    let b = second.alloc(Span::new(0, 4));

    // Act — 逆順に差し込む
    let mut map = SourceMap::default();
    map.insert(second);
    map.insert(first);

    // Assert
    assert_eq!(map.location(a).span, Span::new(0, 1));
    assert_eq!(map.location(a).source_id, SourceId::new(0));
    assert_eq!(map.location(b).span, Span::new(0, 4));
    assert_eq!(map.location(b).source_id, SourceId::new(1));
  }

  #[test]
  fn get_returns_none_for_unregistered_source() {
    // Arrange
    let mut spans = SourceSpans::new(SourceId::new(2));
    let id = spans.alloc(Span::new(0, 1));

    // Act — SourceId(2) を登録しないまま引く
    let map = SourceMap::default();

    // Assert
    assert!(map.get(id).is_none());
  }
}
