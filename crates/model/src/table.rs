//! 表（`table` 環境）の行・セルを表す型

use crate::InlineNode;

// =============================================================================
// 表関連の型
// =============================================================================

/// 表の 1 行（`\row{...}` に対応）
///
/// ヘッダ行（`\head` 内）と本体行の双方で使われる。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
  /// 行内のセル（ソース上の `&` 区切り、または `\cell[...]{...}`）
  pub cells: Vec<TableCell>,
  /// この行の上に横罫線を引くか（`\row[rule_above]{...}`）
  pub rule_above: bool,
}

/// 表の 1 セル
///
/// 通常セル（`&` 区切りの内容）は `span = 1`。
/// `\cell[span=N]{...}` で列方向の結合を指定できる。
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
  /// セルの内容（インライン要素）
  pub content: Vec<InlineNode>,
  /// 列方向の結合数（colspan、1 以上）
  pub span: u32,
}

impl TableCell {
  /// span = 1 の通常セルを生成する
  #[must_use]
  pub fn new(content: Vec<InlineNode>) -> Self { return TableCell { content, span: 1 }; }
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn table_cell_new_defaults_to_span_one() {
    // TableCell::new は span = 1 の通常セルを作り、内容を保持する
    let cell = TableCell::new(vec![InlineNode::text("a")]);
    assert_eq!(cell.span, 1);
    assert_eq!(cell.content.len(), 1);
  }

  #[test]
  fn table_cell_supports_explicit_span() {
    // \cell[span=N] 相当の明示 span を保持できる
    let cell = TableCell {
      content: vec![InlineNode::text("x")],
      span: 3,
    };
    assert_eq!(cell.span, 3);
  }

  #[test]
  fn table_row_holds_cells_and_rule_above() {
    // TableRow はセル列と rule_above フラグをそのまま保持する
    let row = TableRow {
      cells: vec![
        TableCell::new(vec![InlineNode::text("a")]),
        TableCell::new(vec![InlineNode::text("b")]),
      ],
      rule_above: true,
    };
    assert_eq!(row.cells.len(), 2);
    assert!(row.rule_above);
  }
}
