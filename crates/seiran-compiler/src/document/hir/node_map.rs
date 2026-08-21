//! `NodeId` をキーにする挿入順の side table [`NodeMap`]。
//!
//! 「全ノード数ぶんの `Option<T>`」を露出させないため、fact を持つノードだけを保持する。
//! 走査順（= 文書順）に依存する利用者（CSL の採番）のため、`iter` は挿入順を保つ。

use std::collections::HashMap;

use crate::document::hir::NodeId;

/// `NodeId` → 値の side table（挿入順を保つ）
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeMap<T> {
  /// 挿入順のエントリ列
  entries: Vec<(NodeId, T)>,
  /// `NodeId` から `entries` の添字を引く索引
  index: HashMap<NodeId, usize>,
}

impl<T> Default for NodeMap<T> {
  fn default() -> Self {
    return NodeMap {
      entries: Vec::new(),
      index: HashMap::new(),
    };
  }
}

impl<T> NodeMap<T> {
  /// `id` に値を対応づける（同じ `id` の再挿入は値を置き換え、順序は最初の位置のまま）
  pub(crate) fn insert(&mut self, id: NodeId, value: T) {
    if let Some(&position) = self.index.get(&id) {
      self.entries[position].1 = value;
    } else {
      self.index.insert(id, self.entries.len());
      self.entries.push((id, value));
    }
    return;
  }

  /// `id` に対応する値を返す（未登録なら `None`）
  pub(crate) fn get(&self, id: NodeId) -> Option<&T> {
    let position = self.index.get(&id)?;
    return self.entries.get(*position).map(|(_, value)| return value);
  }

  /// エントリを挿入順に走査する
  pub(crate) fn iter(&self) -> impl Iterator<Item = (NodeId, &T)> {
    return self.entries.iter().map(|(id, value)| return (*id, value));
  }

  /// エントリ数を返す
  #[cfg(test)]
  pub(crate) fn len(&self) -> usize { return self.entries.len(); }

  /// エントリが 1 つも無いかを返す
  pub(crate) fn is_empty(&self) -> bool { return self.entries.is_empty(); }
}

#[cfg(test)]
mod tests {
  use super::NodeMap;
  use crate::{document::hir::NodeId, source::SourceId};

  /// テスト用の `NodeId` を作る
  fn id(source: usize, local: u32) -> NodeId { return NodeId::for_test(SourceId::new(source), local); }

  #[test]
  fn node_map_iterates_in_insertion_order() {
    // Arrange
    let mut map: NodeMap<&str> = NodeMap::default();

    // Act — 意図的に local の降順・ソース跨ぎで入れる
    map.insert(id(1, 7), "c");
    map.insert(id(0, 9), "a");
    map.insert(id(0, 2), "b");

    // Assert
    let values: Vec<&str> = map.iter().map(|(_, value)| return *value).collect();
    assert_eq!(values, vec!["c", "a", "b"], "iter は挿入順（走査順）を保つはず");
    assert_eq!(map.len(), 3);
  }

  #[test]
  fn node_map_gets_value_by_id() {
    // Arrange
    let mut map: NodeMap<u32> = NodeMap::default();
    map.insert(id(0, 1), 10);
    map.insert(id(1, 1), 20);

    // Act / Assert — ソースが違えば別キー
    assert_eq!(map.get(id(0, 1)), Some(&10));
    assert_eq!(map.get(id(1, 1)), Some(&20));
    assert_eq!(map.get(id(2, 1)), None, "未登録は None");
  }

  #[test]
  fn node_map_is_empty_by_default() {
    // Arrange / Act
    let map: NodeMap<u32> = NodeMap::default();

    // Assert
    assert!(map.is_empty());
  }
}
