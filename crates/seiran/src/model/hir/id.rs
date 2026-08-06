//! HIR ノードの識別子 [`NodeId`]。

use crate::source::SourceId;

/// 1 回のコンパイル内で HIR ノードを一意に識別する ID
///
/// 永続化形式ではなく 1 回の `compile` 内でのみ有効。発行できるのは `HirBuilder` だけで、
/// 発行と同時に `SourceSpans` へ位置が記録されるため「位置を持たない `NodeId`」は構築できない。
///
/// `local` は各ソース内で独立に採番する（スレッド共有カウンタを使わない）ので、
/// 複数ソースをどの順序でパースしても同じ ID になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeId {
  /// この ID が属するソース
  source: SourceId,
  /// ソース内の連番（ソース出現順＝ preorder。捨てられた予約により穴が空きうる）
  local: u32,
}

impl NodeId {
  /// `SourceSpans` だけが呼べる構築子
  pub(super) fn new(source: SourceId, local: u32) -> Self { return NodeId { source, local }; }

  /// 属するソースを返す
  pub(crate) fn source(self) -> SourceId { return self.source; }

  /// ソース内の連番を返す
  pub(crate) fn local(self) -> u32 { return self.local; }

  /// テスト専用の構築子（本体コードは `HirBuilder` 経由でしか ID を得られない）
  #[cfg(test)]
  pub(crate) fn for_test(source: SourceId, local: u32) -> Self { return NodeId { source, local }; }
}
