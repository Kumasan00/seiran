//! HIR ノードの ID 発行と位置記録を行う、1 ソースぶんの構築 context [`HirBuilder`]。

use std::cell::RefCell;

use crate::{
  document::hir::{HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, NodeId, SourceSpans},
  source::{SourceId, Span},
};

/// HIR ノードの ID を発行し、同時にソース位置を記録する builder
///
/// 外部資源パス（`\image{...}`）の解決はここには無い。解決規則を持つのは `project::PathResolver` で、
/// 評価中に builder と resolver を束ねて持ち回るのは frontend の評価 context である。
/// この型は文書構築の不変条件（ID・位置・leaf ノード）だけを持つ。
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
