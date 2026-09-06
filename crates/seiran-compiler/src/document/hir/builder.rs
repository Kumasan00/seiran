//! HIR ノードの ID 発行・位置記録・外部資源パスの解決を行う、1 ソースぶんの構築 context [`HirBuilder`]。

use std::{cell::RefCell, path::Path};

use crate::{
  document::hir::{HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, NodeId, SourceSpans},
  project::{PathResolver, ProjectPath},
  source::{SourceId, Span},
};

/// HIR ノードの ID を発行し、同時にソース位置を記録し、ソースに書かれた外部資源パスを解決する builder
///
/// `NodeId` を発行できる唯一の型。ID 発行と位置記録が同じ呼び出しで起きるため、
/// 「位置を持たない `NodeId`」は構築できない。
///
/// 外部資源パス（`\image{...}`）の解決もここが受け持つ — 環境ハンドラは全部 `fn(view, &HirBuilder)`
/// の phf テーブルで dispatch されるので、resolver を別の context 型に束ねると fn ポインタ型と
/// 画像を扱わないハンドラの interface まで変わる。builder は「1 ソースぶんの HIR 構築 context」として
/// 全ハンドラが既に受け取っており、ここに載せるのが最小で凝集する（#530）。解決規則の実装は
/// [`PathResolver`]（`project`）の 1 箇所だけで、frontend は `base_dir.join` を書かない。
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
  /// ソースに書かれた外部資源パスを `ProjectPath` へ解決する規則（`compile` facade が 1 回構築した値の複製）
  resolver: PathResolver,
}

impl HirBuilder {
  /// 指定ソース向けの builder を作る
  pub(crate) fn new(source_id: SourceId, resolver: PathResolver) -> Self {
    return HirBuilder {
      spans: RefCell::new(SourceSpans::new(source_id)),
      resolver,
    };
  }

  /// ソースに書かれた外部資源のパスを、`base_dir` 基準の正規化済み [`ProjectPath`] へ解決する
  ///
  /// HIR へ格納する時点で解決するので、後段が文書木を走査して書き戻す解決 pass は要らない。
  pub(crate) fn resolve_path(&self, path: impl AsRef<Path>) -> ProjectPath { return self.resolver.resolve(path); }

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
mod tests {
  use super::*;
  use crate::document::MathClass;

  /// 空の `base_dir` の builder（このテストはパス解決を見ない）
  fn builder() -> HirBuilder { return HirBuilder::new(SourceId::new(0), PathResolver::new(Path::new(""))); }

  #[test]
  fn alloc_before_children_yields_preorder_locals() {
    // Arrange
    let builder = builder();

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
    let builder = builder();

    // Act
    let outer = builder.alloc(Span::new(0, 9));
    let inner = builder.leaf_math(
      Span::new(1, 2),
      HirMathKind::Symbol {
        ch: 'x',
        class: MathClass::Ord,
      },
    );
    builder.set_span(outer, Span::new(0, 12));

    // Assert
    assert_eq!(builder.span_of(outer), Span::new(0, 12));
    assert_eq!(builder.span_of(inner.id), Span::new(1, 2));
  }

  #[test]
  fn finish_returns_all_allocated_spans() {
    // Arrange
    let builder = builder();
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

  #[test]
  fn resolve_path_applies_the_resolver_base_dir() {
    let builder = HirBuilder::new(SourceId::new(0), PathResolver::new(Path::new("/project")));

    assert_eq!(builder.resolve_path("fig/a.png"), ProjectPath::new("/project/fig/a.png"));
  }
}
