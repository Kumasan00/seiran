//! リスト（`DocNode::List`）の lowering

use document::ListItem;

use super::{LoweringContext, LoweringError, lower_nodes};
use crate::layout_node::{LayoutNode, TextStyle};

/// リストをレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] リストのネスト対応（ネストレベルに応じたインデント量の変更）
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[ListItem],
) -> Result<Vec<LayoutNode>, LoweringError> {
  let list_style = &ctx.style.core.list;
  let mut result = Vec::new();

  let marker_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: list_style.marker_font_kind,
  };

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成
    // TODO: ネストレベルに応じたマーカーの変更
    let marker = if ordered {
      format!("{} ", list_style.ordered_format.replace("{number}", &(i + 1).to_string()))
    } else {
      format!("{} ", list_style.unordered_marker)
    };

    // インデント + マーカー + 内容
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Kern {
      length: list_style.indent,
    });
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes(ctx, &item.content)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: list_style.item_margin_bottom,
    });
  }

  return Ok(result);
}

#[cfg(test)]
mod tests {
  use document::{DocNode, InlineNode, ListItem};
  use read_style::Style as ReadStyle;
  use types::FontKind;

  use super::*;

  /// テキスト 1 段落だけを内容に持つ `ListItem` を作るヘルパ
  fn item_with_text(text: &str) -> ListItem {
    return ListItem::new(vec![DocNode::Paragraph(vec![InlineNode::Text(text.to_string())])]);
  }

  /// `lower_list` が返す各 item（`VBox`）から先頭の `Kern` 長とマーカー `Text` を取り出す
  fn kern_and_marker(node: &LayoutNode) -> (f32, &str, TextStyle) {
    let LayoutNode::VBox { children, .. } = node else {
      panic!("item は VBox であるべき: {node:?}");
    };
    let LayoutNode::Kern { length } = &children[0] else {
      panic!("先頭は Kern（インデント）であるべき: {children:?}");
    };
    let LayoutNode::Text(marker, style) = &children[1] else {
      panic!("2 番目はマーカー Text であるべき: {children:?}");
    };
    return (length.to_pt(), marker, *style);
  }

  #[test]
  fn unordered_list_uses_marker_with_trailing_space() {
    // Arrange — 既定 unordered_marker は "•"
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("apple")];

    // Act
    let nodes = lower_list(&ctx, false, &items).expect("解決済み内容なので失敗しない");

    // Assert — マーカーは "• "（末尾スペース付き）
    let (_, marker, _) = kern_and_marker(&nodes[0]);
    assert_eq!(marker, "• ");
  }

  #[test]
  fn ordered_list_numbers_start_at_one_and_increment() {
    // Arrange — 既定 ordered_format は "{number}."
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];

    // Act
    let nodes = lower_list(&ctx, true, &items).expect("失敗しない");

    // Assert — 1,2,3 と連番で展開され、末尾スペースが付く
    assert_eq!(nodes.len(), 3);
    let markers: Vec<&str> = nodes.iter().map(|n| kern_and_marker(n).1).collect();
    assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
  }

  #[test]
  fn item_vbox_uses_indent_margin_and_marker_style() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("x")];

    // Act
    let nodes = lower_list(&ctx, false, &items).expect("失敗しない");

    // Assert — Kern=indent、VBox.margin_bottom=item_margin_bottom、marker_style は規定どおり
    let list_style = &style.core.list;
    let (indent_pt, _, marker_style) = kern_and_marker(&nodes[0]);
    assert!((indent_pt - list_style.indent.to_pt()).abs() < f32::EPSILON);
    let LayoutNode::VBox { margin_bottom, .. } = &nodes[0] else {
      panic!("item は VBox");
    };
    assert!((margin_bottom.to_pt() - list_style.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
    assert_eq!(marker_style.font_kind, list_style.marker_font_kind);
    assert_eq!(marker_style.font_kind, FontKind::Serif);
    assert!((marker_style.font_size - ctx.default_font_size()).abs() < f32::EPSILON);
  }

  #[test]
  fn empty_items_yield_empty_vec() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_list(&ctx, true, &[]).expect("失敗しない");

    // Assert
    assert!(nodes.is_empty());
  }
}
