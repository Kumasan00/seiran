//! 表（`table` 環境）の行・セルを表す型

use crate::inline::InlineNode;

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
