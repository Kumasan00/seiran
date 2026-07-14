//! 箇条書きリストのアイテムを表す型

use crate::DocNode;

// =============================================================================
// リスト関連の型
// =============================================================================

/// リストの個別アイテム（`\item` に対応）
///
/// 各アイテムは複数の `DocNode` を含むことができ、
/// 段落やネストされたリストを内包できます。
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
  /// アイテムの内容（段落、ネストされたリスト等）
  pub content: Vec<DocNode>,
}

impl ListItem {
  /// 新しい `ListItem` を生成する
  #[must_use]
  pub fn new(content: Vec<DocNode>) -> Self { return ListItem { content }; }
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;
  use crate::InlineNode;

  #[test]
  fn list_item_new() {
    let item = ListItem::new(vec![DocNode::Paragraph(vec![InlineNode::text("item")])]);
    assert_eq!(item.content.len(), 1);
  }
}
