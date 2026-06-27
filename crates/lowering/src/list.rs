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
  let list_style = &ctx.style.list;
  let mut result = Vec::new();

  // 項目内容には本文の段落先頭字下げを波及させない（マーカー直後への字下げを避ける）。
  let item_ctx = ctx.with_first_line_indent(types::Length::pt(0.0));

  let marker_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: list_style.marker_font_kind,
    color: None,
  };

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成
    // TODO: ネストレベルに応じたマーカーの変更
    let marker = if ordered {
      format!("{} ", list_style.ordered_marker_format.replace("{number}", &(i + 1).to_string()))
    } else {
      format!("{} ", list_style.unordered_marker)
    };

    // マーカー + 内容。左インデントは VBox.indent（ブロック単位）で表し、折り返し行・
    // ネストにも一律適用する。マーカーは先頭行の行頭インラインとして置く。
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes(&item_ctx, &item.content)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: list_style.item_margin_bottom,
      indent: list_style.indent,
      right_indent: types::Length::pt(0.0),
      align: types::Align::Left,
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

  /// `lower_list` が返す各 item（`VBox`）から先頭のマーカー `Text` を取り出す
  ///
  /// インデントは `VBox.indent`（ブロック単位）で表すため、項目の先頭インラインはマーカー。
  fn marker_of(node: &LayoutNode) -> (&str, TextStyle) {
    let LayoutNode::VBox { children, .. } = node else {
      panic!("item は VBox であるべき: {node:?}");
    };
    let LayoutNode::Text(marker, style) = &children[0] else {
      panic!("先頭はマーカー Text であるべき: {children:?}");
    };
    return (marker, *style);
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
    let (marker, _) = marker_of(&nodes[0]);
    assert_eq!(marker, "• ");
  }

  #[test]
  fn ordered_list_numbers_start_at_one_and_increment() {
    // Arrange — 既定 ordered_marker_format は "{number}."
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
    let markers: Vec<&str> = nodes.iter().map(|n| marker_of(n).0).collect();
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

    // Assert — VBox.indent=indent（ブロック単位）・先頭に Kern は出さない、
    // margin_bottom=item_margin_bottom、marker_style は規定どおり
    let list_style = &style.list;
    let (_, marker_style) = marker_of(&nodes[0]);
    let LayoutNode::VBox {
      children,
      margin_bottom,
      indent,
      ..
    } = &nodes[0]
    else {
      panic!("item は VBox");
    };
    assert!(!matches!(children[0], LayoutNode::Kern { .. }), "先頭に Kern は出さない: {children:?}");
    assert!((indent.to_pt() - list_style.indent.to_pt()).abs() < f32::EPSILON);
    assert!((margin_bottom.to_pt() - list_style.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
    assert_eq!(marker_style.font_kind, list_style.marker_font_kind);
    assert_eq!(marker_style.font_kind, FontKind::Serif);
    assert!((marker_style.font_size - ctx.default_font_size()).abs() < f32::EPSILON);
  }

  #[test]
  fn nested_list_item_also_carries_indent() {
    // Arrange — 項目内容にネストしたリストを含める。各段の item VBox に indent が乗ることで、
    // build_blocks 側で外側 indent と累積され、ネスト項目が段ごとに深く字下げされる。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = DocNode::List {
      ordered: false,
      items: vec![ListItem::new(vec![DocNode::Paragraph(vec![
        InlineNode::Text("inner".to_string()),
      ])])],
    };
    let items = [ListItem::new(vec![
      DocNode::Paragraph(vec![InlineNode::Text("outer".to_string())]),
      nested,
    ])];

    // Act
    let nodes = lower_list(&ctx, false, &items).expect("失敗しない");

    // Assert — 外側 item VBox.indent == list.indent、内側ネスト item VBox にも同じ indent が乗る
    let indent = style.list.indent.to_pt();
    let LayoutNode::VBox {
      children,
      indent: outer_indent,
      ..
    } = &nodes[0]
    else {
      panic!("外側 item は VBox");
    };
    assert!((outer_indent.to_pt() - indent).abs() < f32::EPSILON);
    let nested_indent = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { indent, .. } => Some(indent.to_pt()),
        _ => None,
      })
      .expect("ネストした item VBox があるはず");
    assert!((nested_indent - indent).abs() < f32::EPSILON, "ネスト item にも indent が乗る");
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
