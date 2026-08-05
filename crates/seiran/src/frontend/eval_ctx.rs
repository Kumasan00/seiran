//! 評価中に持ち回すコンテキスト [`EvalCtx`]。
// 評価器への接続は #322 の後続 task（数式・インライン・ブロックの HIR 化）で行う。
// それまでは未使用に見えるため、この module 単位で抑制する（接続時に外す）。
#![allow(dead_code)]

use crate::model::{HirBuilder, NodeId, Span};

/// ソーステキストと HIR builder の組
///
/// 評価器の各関数は `source: &str` の代わりにこれを受け取り、トークンのテキスト取得と
/// HIR ノードの ID 発行の両方をここから行う。`Copy` なので参照ではなく値で渡す。
#[derive(Debug, Clone, Copy)]
pub(crate) struct EvalCtx<'a> {
  /// 元のソーステキスト
  source: &'a str,
  /// ID 発行と位置記録を行う builder
  builder: &'a HirBuilder,
}

impl<'a> EvalCtx<'a> {
  /// コンテキストを作る
  pub(crate) fn new(source: &'a str, builder: &'a HirBuilder) -> Self { return EvalCtx { source, builder }; }

  /// 元のソーステキストを返す
  pub(crate) fn source(self) -> &'a str { return self.source; }

  /// builder を返す
  pub(crate) fn builder(self) -> &'a HirBuilder { return self.builder; }

  /// 新しい ID を発行し、`span` を記録する（`builder().alloc(span)` の短縮）
  pub(crate) fn alloc(self, span: Span) -> NodeId { return self.builder.alloc(span); }
}
