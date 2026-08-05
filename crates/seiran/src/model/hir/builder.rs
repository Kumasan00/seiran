//! HIR ノードの ID 発行と位置記録を行う [`HirBuilder`]。

use std::cell::RefCell;

use crate::model::{
  SourceId, Span,
  hir::{HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, NodeId, SourceSpans},
};

/// HIR ノードの ID を発行し、同時にソース位置を記録する builder
///
/// `NodeId` を発行できる唯一の型。ID 発行と位置記録が同じ呼び出しで起きるため、
/// 「位置を持たない `NodeId`」は構築できない。
///
/// 子を持つノードは、子を評価する**前**に [`HirBuilder::alloc`] で自分の ID を確保すること。
/// `NodeId::local` がソース出現順（preorder）になるのはこの規約だけで成り立つ。
/// 子を持たないノードには [`HirBuilder::leaf_node`] 等を使う。
///
/// 評価器は再帰の途中でこの builder を共有するため、内部可変（`RefCell`）で借用を
/// 各メソッド内に閉じる。
#[derive(Debug)]
pub(crate) struct HirBuilder {
  /// 発行済み ID と位置。借用は各メソッド内で閉じ、再帰評価をまたいで保持しない
  spans: RefCell<SourceSpans>,
}

impl HirBuilder {
  /// 指定ソース向けの builder を作る
  pub(crate) fn new(source_id: SourceId) -> Self {
    return HirBuilder {
      spans: RefCell::new(SourceSpans::new(source_id)),
    };
  }

  /// 新しい ID を発行し、`span` を記録する
  ///
  /// 子を持つノードの ID 確保（予約）にも使う。
  pub(crate) fn alloc(&self, span: Span) -> NodeId { return self.spans.borrow_mut().alloc(span); }

  /// 予約済み ID の span を確定させる
  ///
  /// 段落のように、確保した時点では閉じ位置が決まらないノードで使う。
  pub(crate) fn set_span(&self, id: NodeId, span: Span) {
    self.spans.borrow_mut().set_span(id, span);
    return;
  }

  /// 発行済み ID の span を返す
  pub(crate) fn span_of(&self, id: NodeId) -> Span { return self.spans.borrow().span_of(id); }

  /// 子を持たないブロックノードを 1 回で作る
  pub(crate) fn leaf_node(&self, span: Span, kind: HirNodeKind) -> HirNode {
    return HirNode::new(self.alloc(span), kind);
  }

  /// 子を持たないインラインノードを 1 回で作る
  pub(crate) fn leaf_inline(&self, span: Span, kind: HirInlineKind) -> HirInline {
    return HirInline::new(self.alloc(span), kind);
  }

  /// 子を持たない数式ノードを 1 回で作る
  pub(crate) fn leaf_math(&self, span: Span, kind: HirMathKind) -> HirMath {
    return HirMath::new(self.alloc(span), kind);
  }

  /// 位置表を取り出して builder を終える
  pub(crate) fn finish(self) -> SourceSpans { return self.spans.into_inner(); }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;

  #[test]
  fn alloc_before_children_yields_preorder_locals() {
    // Arrange
    let builder = HirBuilder::new(SourceId::new(0));

    // Act — 親を先に確保してから子を作る
    let parent = builder.alloc(Span::new(0, 10));
    let child = builder.leaf_inline(Span::new(3, 7), HirInlineKind::Text("abc".to_string()));

    // Assert
    assert!(parent.local() < child.id.local());
    assert_eq!(builder.span_of(parent), Span::new(0, 10));
    assert_eq!(builder.span_of(child.id), Span::new(3, 7));
  }

  #[test]
  fn alloc_during_nested_use_does_not_panic() {
    // Arrange — 再帰評価中に借用が重ならないことの回帰テスト
    let builder = HirBuilder::new(SourceId::new(0));

    // Act
    let outer = builder.alloc(Span::new(0, 9));
    let inner = builder.leaf_math(Span::new(1, 2), HirMathKind::Symbol('x'));
    builder.set_span(outer, Span::new(0, 12));

    // Assert
    assert_eq!(builder.span_of(outer), Span::new(0, 12));
    assert_eq!(builder.span_of(inner.id), Span::new(1, 2));
  }

  #[test]
  fn finish_returns_all_allocated_spans() {
    // Arrange
    let builder = HirBuilder::new(SourceId::new(0));
    let first = builder.alloc(Span::new(0, 1));
    let second = builder.alloc(Span::new(1, 2));

    // Act
    let spans = builder.finish();

    // Assert
    assert_eq!(spans.len(), 2);
    assert_eq!(spans.span_of(first), Span::new(0, 1));
    assert_eq!(spans.span_of(second), Span::new(1, 2));
    assert_eq!(spans.source_id(), SourceId::new(0));
  }
}
