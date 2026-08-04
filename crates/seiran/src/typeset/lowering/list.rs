//! リスト（`resolve::ResolvedNode::List`）の lowering

use model::Length;

use super::{
  LoweringContext, LoweringState,
  layout_node::{LayoutNode, TextStyle},
  lower_nodes_inner,
};
use crate::resolve::ResolvedListItem;

/// リストをレイアウトノードに変換する
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[ResolvedListItem],
  start: Option<u32>,
  item_gap: Option<Length>,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let list_style = &ctx.style.list;
  let depth = ctx.list_depth;
  let mut result = Vec::new();

  // 項目内容には本文の段落先頭字下げを波及させない（マーカー直後への字下げを避ける）。
  // 同時にネスト深さを +1 して渡し、item 内容中のネストしたリストが深さ +1 で lower されるようにする。
  let item_ctx = ctx.with_first_line_indent(model::Length::pt(0.0)).with_list_depth(depth + 1);

  let marker_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: list_style.marker_font_kind,
    color: None,
  };

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成。`\item[marker=...]` による個別上書きがあれば自動生成より優先する。
    // 連番カウンタ n はこの上書きと独立に i から算出するため、上書きされた項目があっても
    // 後続項目の自動番号はズレない。
    let base = start.unwrap_or(1);
    let offset = u32::try_from(i).expect("リスト項目数は u32 に収まる前提");
    let n = base.saturating_add(offset);
    let marker_body = if let Some(marker) = &item.marker {
      marker.clone()
    } else if ordered {
      if depth == 0 {
        list_style.ordered_marker_format.replace("{number}", &n.to_string())
      } else {
        let format = &list_style.nested_ordered_formats[(depth - 1) % list_style.nested_ordered_formats.len()];
        format.format.replace("{number}", &format.number_style.render(n))
      }
    } else if depth == 0 {
      list_style.unordered_marker.clone()
    } else {
      list_style.nested_unordered_markers[(depth - 1) % list_style.nested_unordered_markers.len()].clone()
    };

    // マーカー + 内容。左インデントは VBox.indent（ブロック単位）で表し、折り返し行・
    // ネストにも一律適用する。マーカーは先頭行の行頭インラインとして置く。`marker=""` の
    // 明示指定時（marker_body が空）はマーカー Text 自体を出さず、ぶら下げインデントのみにする。
    let mut item_nodes = Vec::new();
    if !marker_body.is_empty() {
      item_nodes.push(LayoutNode::Text(format!("{marker_body} "), marker_style));
    }

    let content_nodes = lower_nodes_inner(&item_ctx, &item.content, state);
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: item.item_gap.or(item_gap).unwrap_or(list_style.item_margin_bottom),
      indent: list_style.indent,
      right_indent: model::Length::pt(0.0),
      align: model::Align::Left,
    });
  }

  return result;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use model::FontKind;

  use super::{super::test_support, *};
  use crate::{
    config::Style as ReadStyle,
    resolve::{ResolvedInline, ResolvedNode},
  };

  /// テキスト 1 段落だけを内容に持つ `ResolvedListItem` を作るヘルパ
  fn item_with_text(text: &str) -> ResolvedListItem {
    return ResolvedListItem {
      content: vec![ResolvedNode::Paragraph(vec![ResolvedInline::Text(
        text.to_string(),
      )])],
      marker: None,
      item_gap: None,
    };
  }

  /// 任意の内容ノード列を持つ `ResolvedListItem` を作るヘルパ
  fn item_with_content(content: Vec<ResolvedNode>) -> ResolvedListItem {
    return ResolvedListItem {
      content,
      marker: None,
      item_gap: None,
    };
  }

  /// テスト用に `LoweringState` を構築して `lower_list` を呼ぶヘルパ
  fn lower_list_default(
    ctx: &LoweringContext,
    ordered: bool,
    items: &[ResolvedListItem],
    start: Option<u32>,
  ) -> Vec<LayoutNode> {
    return lower_list_with_gap(ctx, ordered, items, start, None);
  }

  /// `item_gap`（環境単位の縦アキ上書き）も指定できるテストヘルパ
  fn lower_list_with_gap(
    ctx: &LoweringContext,
    ordered: bool,
    items: &[ResolvedListItem],
    start: Option<u32>,
    item_gap: Option<model::Length>,
  ) -> Vec<LayoutNode> {
    let document = test_support::document(&[]);
    return lower_list(ctx, ordered, items, start, item_gap, &mut LoweringState::new(&document));
  }

  /// `lower_list` が返す各 item（`VBox`）から先頭のマーカー `Text` を取り出す
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
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("apple")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None);

    // Assert
    let (marker, _) = marker_of(&nodes[0]);
    assert_eq!(marker, "• ");
  }

  #[test]
  fn ordered_list_numbers_start_at_one_and_increment() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];

    // Act
    let nodes = lower_list_default(&ctx, true, &items, None);

    // Assert
    assert_eq!(nodes.len(), 3);
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
  }

  #[test]
  fn ordered_list_with_start_numbers_from_start() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];

    // Act
    let nodes = lower_list_default(&ctx, true, &items, Some(5));

    // Assert
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["5. ", "6. ", "7. "]);
  }

  #[test]
  fn nested_start_does_not_affect_outer_numbering() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = ResolvedNode::List {
      ordered: true,
      items: vec![item_with_text("inner-a"), item_with_text("inner-b")],
      start: Some(10),
      item_gap: None,
    };
    let items = [
      item_with_text("outer-a"),
      item_with_content(vec![
        ResolvedNode::Paragraph(vec![ResolvedInline::Text("outer-b".to_string())]),
        nested,
      ]),
    ];

    // Act
    let nodes = lower_list_default(&ctx, true, &items, None);

    // Assert
    let outer_markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(outer_markers, vec!["1. ", "2. "]);
    let inner_vbox = first_nested_vbox(&nodes[1]);
    assert_eq!(marker_of(inner_vbox).0, "(j) ");
  }

  #[test]
  fn sibling_list_after_start_list_is_unaffected() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let doc_nodes = vec![
      ResolvedNode::List {
        ordered: true,
        items: vec![item_with_text("a"), item_with_text("b")],
        start: Some(5),
        item_gap: None,
      },
      ResolvedNode::List {
        ordered: true,
        items: vec![item_with_text("x")],
        start: None,
        item_gap: None,
      },
    ];

    // Act
    let document = test_support::document(&[]);
    let nodes = super::super::lower_nodes_inner(&ctx, &doc_nodes, &mut LoweringState::new(&document));

    // Assert
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["5. ", "6. ", "1. "]);
  }

  #[test]
  fn item_vbox_uses_indent_margin_and_marker_style() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("x")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None);

    // Assert
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
    assert_eq!(marker_style.font_size, ctx.default_font_size());
  }

  #[test]
  fn nested_list_item_also_carries_indent() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = ResolvedNode::List {
      ordered: false,
      items: vec![item_with_content(vec![ResolvedNode::Paragraph(vec![
        ResolvedInline::Text("inner".to_string()),
      ])])],
      start: None,
      item_gap: None,
    };
    let items = [item_with_content(vec![
      ResolvedNode::Paragraph(vec![ResolvedInline::Text("outer".to_string())]),
      nested,
    ])];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None);

    // Assert
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
        LayoutNode::VBox { indent, .. } => return Some(indent.to_pt()),
        _ => return None,
      })
      .expect("ネストした item VBox があるはず");
    assert!((nested_indent - indent).abs() < f32::EPSILON, "ネスト item にも indent が乗る");
  }

  #[test]
  fn item_marker_override_replaces_auto_marker() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];
    items[1].marker = Some("Q1.".to_string());

    // Act
    let nodes = lower_list_default(&ctx, true, &items, None);

    // Assert
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "Q1. ", "3. "]);
  }

  #[test]
  fn item_marker_override_empty_string_omits_marker_text() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut items = [item_with_text("x")];
    items[0].marker = Some(String::new());

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None);

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[0] else {
      panic!("item は VBox であるべき: {:?}", nodes[0]);
    };
    let LayoutNode::Text(text, _) = &children[0] else {
      panic!("先頭は内容の Text であるべき: {children:?}");
    };
    assert_eq!(text, "x", "マーカー Text を挟まず内容の Text から始まるべき");
  }

  #[test]
  fn empty_items_yield_empty_vec() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_list_default(&ctx, true, &[], None);

    // Assert
    assert!(nodes.is_empty());
  }

  /// 種別（ordered フラグ）の列から、各段 1 項目のネストしたリストの items を構築する
  fn build_nested_items(kinds: &[bool]) -> Vec<ResolvedListItem> {
    let mut content = vec![ResolvedNode::Paragraph(vec![ResolvedInline::Text(
      "x".to_string(),
    )])];
    if kinds.len() > 1 {
      content.push(ResolvedNode::List {
        ordered: kinds[1],
        items: build_nested_items(&kinds[1..]),
        start: None,
        item_gap: None,
      });
    }
    return vec![item_with_content(content)];
  }

  /// item `VBox` の子から、ネストしたリストの先頭項目 `VBox` を取り出す
  fn first_nested_vbox(node: &LayoutNode) -> &LayoutNode {
    let LayoutNode::VBox { children, .. } = node else {
      panic!("item は VBox であるべき: {node:?}");
    };
    return children
      .iter()
      .find(|c| matches!(c, LayoutNode::VBox { .. }))
      .expect("ネストしたリストの item VBox があるはず");
  }

  /// ネストしたリストを `depth` 段辿り、各段の先頭項目のマーカー文字列を外→内の順で集める
  fn markers_along_chain(nodes: &[LayoutNode], depth: usize) -> Vec<String> {
    let mut cur = &nodes[0];
    let mut markers = vec![marker_of(cur).0.to_string()];
    for _ in 1..depth {
      cur = first_nested_vbox(cur);
      markers.push(marker_of(cur).0.to_string());
    }
    return markers;
  }

  #[test]
  fn nested_unordered_markers_vary_by_depth() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false, false, false];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None);

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "– ", "* "]);
  }

  #[test]
  fn nested_ordered_markers_vary_by_depth() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [true, true, true];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None);

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["1. ", "(a) ", "i. "]);
  }

  #[test]
  fn mixed_nesting_advances_each_kind_by_depth() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false, true, false];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None);

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "(a) ", "* "]);
  }

  #[test]
  fn item_gap_env_override_applies_to_all_items() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("a"), item_with_text("b")];

    // Act
    let nodes = lower_list_with_gap(&ctx, false, &items, None, Some(model::Length::mm(3.0)));

    // Assert
    for node in &nodes {
      let LayoutNode::VBox { margin_bottom, .. } = node else {
        panic!("item は VBox であるべき: {node:?}");
      };
      assert!((margin_bottom.to_pt() - model::Length::mm(3.0).to_pt()).abs() < f32::EPSILON);
    }
  }

  #[test]
  fn item_gap_item_override_takes_priority_over_env() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut items = [item_with_text("a"), item_with_text("b")];
    items[0].item_gap = Some(model::Length::mm(1.0));

    // Act
    let nodes = lower_list_with_gap(&ctx, false, &items, None, Some(model::Length::mm(3.0)));

    // Assert
    let LayoutNode::VBox {
      margin_bottom: gap0,
      ..
    } = &nodes[0]
    else {
      panic!("item は VBox であるべき");
    };
    let LayoutNode::VBox {
      margin_bottom: gap1,
      ..
    } = &nodes[1]
    else {
      panic!("item は VBox であるべき");
    };
    assert!((gap0.to_pt() - model::Length::mm(1.0).to_pt()).abs() < f32::EPSILON);
    assert!((gap1.to_pt() - model::Length::mm(3.0).to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn item_gap_unspecified_falls_back_to_style_default() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("a")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None);

    // Assert
    let LayoutNode::VBox { margin_bottom, .. } = &nodes[0] else {
      panic!("item は VBox であるべき");
    };
    assert!((margin_bottom.to_pt() - style.list.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn item_gap_does_not_propagate_to_nested_list() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = ResolvedNode::List {
      ordered: false,
      items: vec![item_with_text("inner")],
      start: None,
      item_gap: None,
    };
    let items = [item_with_content(vec![
      ResolvedNode::Paragraph(vec![ResolvedInline::Text("outer".to_string())]),
      nested,
    ])];

    // Act
    let nodes = lower_list_with_gap(&ctx, false, &items, None, Some(model::Length::mm(5.0)));

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[0] else {
      panic!("外側 item は VBox であるべき");
    };
    let nested_vbox = children
      .iter()
      .find(|c| matches!(c, LayoutNode::VBox { .. }))
      .expect("ネストしたリストの item VBox があるはず");
    let LayoutNode::VBox { margin_bottom, .. } = nested_vbox else {
      panic!("ネスト item は VBox であるべき");
    };
    assert!((margin_bottom.to_pt() - style.list.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn unordered_marker_sequence_cycles_after_fourth_level() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false; 5];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None);

    // Assert
    assert_eq!(markers_along_chain(&nodes, 5), vec!["• ", "– ", "* ", "· ", "– "]);
  }

  #[test]
  fn nested_unordered_markers_use_style_override() {
    // Arrange
    let mut style = ReadStyle::default();
    style.list.nested_unordered_markers = vec!["§".to_string(), "†".to_string()];
    let ctx = LoweringContext::new(&style);
    let kinds = [false, false, false];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None);

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "§ ", "† "]);
  }

  #[test]
  fn nested_ordered_formats_use_style_override() {
    // Arrange
    let mut style = ReadStyle::default();
    style.list.nested_ordered_formats = vec![
      crate::config::NestedOrderedFormat {
        number_style: crate::config::NumberStyle::RomanUpper,
        format: "[{number}]".to_string(),
      },
      crate::config::NestedOrderedFormat {
        number_style: crate::config::NumberStyle::Kanji,
        format: "{number}、".to_string(),
      },
    ];
    let ctx = LoweringContext::new(&style);
    let kinds = [true, true, true];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None);

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["1. ", "[I] ", "一、 "]);
  }
}
