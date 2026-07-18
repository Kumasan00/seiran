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
  /// `\item[marker=...]` で指定された個別マーカー文字列。
  ///
  /// `None` は自動生成マーカー（深さ・番号から算出）を使うことを表す。`Some("")` は
  /// マーカーを表示しない（ぶら下げインデントのみ）ことを表す。
  pub marker: Option<String>,
}

impl ListItem {
  /// 新しい `ListItem` を生成する（マーカーは自動生成）
  #[must_use]
  pub fn new(content: Vec<DocNode>) -> Self {
    return ListItem {
      content,
      marker: None,
    };
  }
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
