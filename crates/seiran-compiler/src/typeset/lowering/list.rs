//! リスト（`document::HirNodeKind::List`）の lowering

use super::{
  LoweringContext, LoweringState,
  layout_node::{LayoutNode, TextStyle},
  lower_nodes_inner,
};
use crate::{document::HirListItem, length::Length, typeset::boxes::Align};

/// リストをレイアウトノードに変換する
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[HirListItem],
  start: Option<u32>,
  item_gap: Option<Length>,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let list_style = &ctx.style.list;
  let depth = ctx.list_depth;
  let mut result = Vec::new();

  // 項目内容には本文の段落先頭字下げを波及させない（マーカー直後への字下げを避ける）。
  // 同時にネスト深さを +1 して渡し、item 内容中のネストしたリストが深さ +1 で lower されるようにする。
  let item_ctx = ctx.with_first_line_indent(crate::length::Length::pt(0.0)).with_list_depth(depth + 1);

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
      right_indent: crate::length::Length::pt(0.0),
      align: Align::Left,
    });
  }

  return result;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{
    super::test_support::{analyzed, lower},
    *,
  };
  use crate::{document::FontKind, style::Style as ReadStyle};

  /// `.sei` ソースを lower してレイアウトノード列を返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  /// 種別（ordered フラグ）の列から、各段 1 項目のネストしたリストのソースを組み立てる
  fn nested_source(kinds: &[bool]) -> String {
    let Some((first, rest)) = kinds.split_first() else {
      return String::new();
    };
    let name = if *first { "enumerate" } else { "itemize" };
    return format!("\\begin{{{name}}}\n\\item{{x\n{}}}\n\\end{{{name}}}\n", nested_source(rest));
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
  fn empty_list_yields_no_layout_nodes() {
    // Arrange — `\item` が 1 つも無いリスト環境はソースとして書ける（パースも通る）
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{itemize}\n\\end{itemize}\n");

    // Assert
    assert!(nodes.is_empty(), "{nodes:?}");
  }

  #[test]
  fn unordered_list_uses_marker_with_trailing_space() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{itemize}\n\\item{apple}\n\\end{itemize}\n");

    // Assert
    let (marker, _) = marker_of(&nodes[0]);
    assert_eq!(marker, "• ");
  }

  #[test]
  fn ordered_list_numbers_start_at_one_and_increment() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{enumerate}\n\\item{a}\n\\item{b}\n\\item{c}\n\\end{enumerate}\n");

    // Assert
    assert_eq!(nodes.len(), 3);
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
  }

  #[test]
  fn ordered_list_with_start_numbers_from_start() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes =
      lower_source(&style, "\\begin{enumerate}[start=5]\n\\item{a}\n\\item{b}\n\\item{c}\n\\end{enumerate}\n");

    // Assert
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["5. ", "6. ", "7. "]);
  }

  #[test]
  fn nested_start_does_not_affect_outer_numbering() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(
      &style,
      "\\begin{enumerate}\n\\item{outer-a}\n\\item{outer-b\n\
       \\begin{enumerate}[start=10]\n\\item{inner-a}\n\\item{inner-b}\n\\end{enumerate}\n}\n\\end{enumerate}\n",
    );

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

    // Act
    let nodes = lower_source(
      &style,
      "\\begin{enumerate}[start=5]\n\\item{a}\n\\item{b}\n\\end{enumerate}\n\n\
       \\begin{enumerate}\n\\item{x}\n\\end{enumerate}\n",
    );

    // Assert
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["5. ", "6. ", "1. "]);
  }

  #[test]
  fn item_vbox_uses_indent_margin_and_marker_style() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{itemize}\n\\item{x}\n\\end{itemize}\n");

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
    assert_eq!(marker_style.font_size, style.text.font_size);
  }

  #[test]
  fn nested_list_item_also_carries_indent() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, &nested_source(&[false, false]));

    // Assert
    let indent = style.list.indent.to_pt();
    let LayoutNode::VBox {
      indent: outer_indent,
      ..
    } = &nodes[0]
    else {
      panic!("外側 item は VBox");
    };
    assert!((outer_indent.to_pt() - indent).abs() < f32::EPSILON);
    let LayoutNode::VBox {
      indent: nested_indent,
      ..
    } = first_nested_vbox(&nodes[0])
    else {
      panic!("ネスト item は VBox");
    };
    assert!((nested_indent.to_pt() - indent).abs() < f32::EPSILON, "ネスト item にも indent が乗る");
  }

  #[test]
  fn item_marker_override_replaces_auto_marker() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes =
      lower_source(&style, "\\begin{enumerate}\n\\item{a}\n\\item[marker=Q1.]{b}\n\\item{c}\n\\end{enumerate}\n");

    // Assert
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "Q1. ", "3. "]);
  }

  #[test]
  fn item_marker_override_empty_string_omits_marker_text() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{itemize}\n\\item[marker=]{x}\n\\end{itemize}\n");

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
  fn nested_unordered_markers_vary_by_depth() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, &nested_source(&[false, false, false]));

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "– ", "* "]);
  }

  #[test]
  fn nested_ordered_markers_vary_by_depth() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, &nested_source(&[true, true, true]));

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["1. ", "(a) ", "i. "]);
  }

  #[test]
  fn mixed_nesting_advances_each_kind_by_depth() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, &nested_source(&[false, true, false]));

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "(a) ", "* "]);
  }

  #[test]
  fn item_gap_env_override_applies_to_all_items() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{itemize}[item_gap=3mm]\n\\item{a}\n\\item{b}\n\\end{itemize}\n");

    // Assert
    for node in &nodes {
      let LayoutNode::VBox { margin_bottom, .. } = node else {
        panic!("item は VBox であるべき: {node:?}");
      };
      assert!((margin_bottom.to_pt() - Length::mm(3.0).to_pt()).abs() < f32::EPSILON);
    }
  }

  #[test]
  fn item_gap_item_override_takes_priority_over_env() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes =
      lower_source(&style, "\\begin{itemize}[item_gap=3mm]\n\\item[item_gap=1mm]{a}\n\\item{b}\n\\end{itemize}\n");

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
    assert!((gap0.to_pt() - Length::mm(1.0).to_pt()).abs() < f32::EPSILON);
    assert!((gap1.to_pt() - Length::mm(3.0).to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn item_gap_unspecified_falls_back_to_style_default() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\begin{itemize}\n\\item{a}\n\\end{itemize}\n");

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

    // Act
    let nodes = lower_source(
      &style,
      "\\begin{itemize}[item_gap=5mm]\n\\item{outer\n\
       \\begin{itemize}\n\\item{inner}\n\\end{itemize}\n}\n\\end{itemize}\n",
    );

    // Assert
    let LayoutNode::VBox { margin_bottom, .. } = first_nested_vbox(&nodes[0]) else {
      panic!("ネスト item は VBox であるべき");
    };
    assert!((margin_bottom.to_pt() - style.list.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn unordered_marker_sequence_cycles_after_fourth_level() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, &nested_source(&[false; 5]));

    // Assert
    assert_eq!(markers_along_chain(&nodes, 5), vec!["• ", "– ", "* ", "· ", "– "]);
  }

  #[test]
  fn nested_unordered_markers_use_style_override() {
    // Arrange
    let mut style = ReadStyle::default();
    style.list.nested_unordered_markers = vec!["§".to_string(), "†".to_string()];

    // Act
    let nodes = lower_source(&style, &nested_source(&[false, false, false]));

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "§ ", "† "]);
  }

  #[test]
  fn nested_ordered_formats_use_style_override() {
    // Arrange
    let mut style = ReadStyle::default();
    style.list.nested_ordered_formats = vec![
      crate::style::NestedOrderedFormat {
        number_style: crate::style::NumberStyle::RomanUpper,
        format: "[{number}]".to_string(),
      },
      crate::style::NestedOrderedFormat {
        number_style: crate::style::NumberStyle::Kanji,
        format: "{number}、".to_string(),
      },
    ];

    // Act
    let nodes = lower_source(&style, &nested_source(&[true, true, true]));

    // Assert
    assert_eq!(markers_along_chain(&nodes, 3), vec!["1. ", "[I] ", "一、 "]);
  }
}
