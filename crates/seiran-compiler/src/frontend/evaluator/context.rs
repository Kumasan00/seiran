//! 評価中に持ち回る context [`EvalContext`] — HIR の構築と外部資源パスの解決を束ねる。

use std::path::Path;

use crate::{
  document::{
    HirBuilder, HirGroup, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, HirSource, NodeId,
  },
  project::{PathResolver, ProjectPath},
  source::{SourceId, Span},
};

/// 1 ソース分の評価 context
///
/// 評価ハンドラが受け取る唯一の context で、[`HirBuilder`]（ID 発行・位置記録）と
/// `PathResolver`（外部資源パスの解決規則）を束ねる。文書構築の不変条件
/// （ID 発行と位置記録が同じ呼び出しで起きる・親の ID を子より先に確保する）を持つのは
/// [`HirBuilder`] のままで、この型は転送するだけで規約を再定義しない。パス解決規則の実装も
/// `project::PathResolver` 1 箇所に閉じており、frontend は `base_dir.join` を書かない。
///
/// 束ねる置き場が frontend なのは、「評価中に持ち回る値」という括りが frontend の関心だから（#534）。
/// `document` は authored HIR と語彙型の所有者であり、`project` の解決規則を抱える理由は
/// その責務からは導けない。ハンドラの dispatch が fn ポインタの phf テーブルであることは
/// 置き場の判断材料にしない — signature の置換は全ハンドラで一様だから。
///
/// builder は 1 ソースに 1 つなので所有し、resolver は `compile` facade が `base_dir` から
/// 1 回だけ構築した値を借用する。
#[derive(Debug)]
pub(crate) struct EvalContext<'a> {
  /// このソースの HIR ノード ID 発行と位置記録
  builder: HirBuilder,
  /// ソースに書かれた外部資源パスの解決規則
  resolver: &'a PathResolver,
}

impl<'a> EvalContext<'a> {
  /// 指定ソース向けの評価 context を作る
  pub(crate) fn new(source_id: SourceId, resolver: &'a PathResolver) -> Self {
    return EvalContext {
      builder: HirBuilder::new(source_id),
      resolver,
    };
  }

  /// ソースに書かれた外部資源のパスを、`base_dir` 基準の正規化済み [`ProjectPath`] へ解決する
  ///
  /// HIR へ格納する時点で解決するので、後段が文書木を走査して書き戻す解決 pass は要らない。
  pub(crate) fn resolve_path(&self, path: impl AsRef<Path>) -> ProjectPath { return self.resolver.resolve(path); }

  /// 新しい ID を発行し `span` を記録する（[`HirBuilder::alloc`] へ委譲）
  pub(crate) fn alloc(&self, span: Span) -> NodeId { return self.builder.alloc(span); }

  /// 予約済み ID の span を確定させる（[`HirBuilder::set_span`] へ委譲）
  pub(crate) fn set_span(&self, id: NodeId, span: Span) {
    self.builder.set_span(id, span);
    return;
  }

  /// 発行済み ID の span を返す（[`HirBuilder::span_of`] へ委譲）
  pub(crate) fn span_of(&self, id: NodeId) -> Span { return self.builder.span_of(id); }

  /// 子を持たないブロックノードを 1 回で作る（[`HirBuilder::leaf_node`] へ委譲）
  pub(crate) fn leaf_node(&self, span: Span, kind: HirNodeKind) -> HirNode {
    return self.builder.leaf_node(span, kind);
  }

  /// 子を持たないインラインノードを 1 回で作る（[`HirBuilder::leaf_inline`] へ委譲）
  pub(crate) fn leaf_inline(&self, span: Span, kind: HirInlineKind) -> HirInline {
    return self.builder.leaf_inline(span, kind);
  }

  /// 子を持たない数式ノードを 1 回で作る（[`HirBuilder::leaf_math`] へ委譲）
  pub(crate) fn leaf_math(&self, span: Span, kind: HirMathKind) -> HirMath {
    return self.builder.leaf_math(span, kind);
  }

  /// 評価し終えたノード列と位置表を 1 ソース分の [`HirSource`] にまとめて context を終える
  ///
  /// 位置表（`SourceSpans`）は `document` の interface に出ていないため、この型の返り値として
  /// 名指しせず [`HirSource`] の一部として運ぶ。
  pub(crate) fn finish(self, nodes: Vec<HirNode>) -> HirSource {
    let spans = self.builder.finish();
    let source_id = spans.source_id();
    return HirSource {
      group: HirGroup { source_id, nodes },
      spans,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resolve_path_applies_the_resolver_base_dir() {
    let resolver = PathResolver::new(Path::new("/project"));
    let ctx = EvalContext::new(SourceId::new(0), &resolver);

    assert_eq!(ctx.resolve_path("fig/a.png"), ProjectPath::new("/project/fig/a.png"));
  }

  #[test]
  fn finish_carries_the_source_id_and_the_spans_of_evaluated_nodes() {
    // Arrange
    let resolver = PathResolver::new(Path::new(""));
    let ctx = EvalContext::new(SourceId::new(3), &resolver);

    // Act
    let node = ctx.leaf_node(Span::new(0, 4), HirNodeKind::Paragraph(Vec::new()));
    let id = node.id;
    let hir = ctx.finish(vec![node]);

    // Assert
    assert_eq!(hir.group.source_id, SourceId::new(3));
    assert_eq!(hir.group.nodes.len(), 1);
    assert_eq!(hir.spans.span_of(id), Span::new(0, 4));
  }
}
