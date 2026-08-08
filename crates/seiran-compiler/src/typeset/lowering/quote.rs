//! 引用ブロック（`document::HirNodeKind::Quote`）の lowering

use super::{LoweringContext, LoweringState, layout_node::LayoutNode, lower_nodes_inner};
use crate::{
  document::{HirNode, QuoteKind},
  length::Length,
  typeset::boxes::Align,
};

/// 引用ブロックをレイアウトノードに変換する
pub(super) fn lower_quote(
  ctx: &LoweringContext,
  kind: QuoteKind,
  body: &[HirNode],
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let style = &ctx.style.quote;

  let first_line_indent = if kind.indents_first_line() {
    style.first_line_indent
  } else {
    Length::pt(0.0)
  };
  let body_ctx = ctx.with_body_font_kind(style.font_kind).with_first_line_indent(first_line_indent);
  let children = lower_nodes_inner(&body_ctx, body, state);

  return vec![
    LayoutNode::Vkern {
      length: style.top_margin,
    },
    LayoutNode::VBox {
      children,
      margin_bottom: Length::pt(0.0),
      indent: style.indent,
      right_indent: style.indent,
      align: Align::Left,
    },
    LayoutNode::Vkern {
      length: style.bottom_margin,
    },
  ];
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{
    super::test_support::{analyzed, lower},
    *,
  };
  use crate::config::Style as ReadStyle;

  /// `quote` / `quotation` 環境 1 つだけの `.sei` ソースを lower するヘルパ
  fn lower_quote_source(style: &ReadStyle, kind: QuoteKind) -> Vec<LayoutNode> {
    let name = match kind {
      QuoteKind::Quote => "quote",
      QuoteKind::Quotation => "quotation",
    };
    let source = format!("\\begin{{{name}}}\nbody\n\\end{{{name}}}\n");
    return lower(style, &analyzed(&source));
  }

  /// `nodes` から本体 `VBox`（`indent` / `right_indent` / `children`）を取り出す
  fn body_vbox(nodes: &[LayoutNode]) -> (Length, Length, &[LayoutNode]) {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox {
          indent,
          right_indent,
          children,
          ..
        } => return Some((*indent, *right_indent, children.as_slice())),
        _ => return None,
      })
      .expect("本体 VBox があるはず");
  }

  #[test]
  fn quote_wraps_body_in_symmetric_indent_vbox_with_margins() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_quote_source(&style, QuoteKind::Quote);

    // Assert
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は top_margin Vkern: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は bottom_margin Vkern: {nodes:?}");
    let (indent, right_indent, _) = body_vbox(&nodes);
    assert!((indent.to_pt() - style.quote.indent.to_pt()).abs() < f32::EPSILON);
    assert!((right_indent.to_pt() - style.quote.indent.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn quote_body_paragraph_has_no_first_line_indent_kern() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_quote_source(&style, QuoteKind::Quote);

    // Assert
    let (_, _, children) = body_vbox(&nodes);
    assert!(
      !children.iter().any(|n| matches!(n, LayoutNode::Kern { .. })),
      "quote に字下げ Kern は出ない: {children:?}"
    );
    assert!(matches!(children.first(), Some(LayoutNode::Text(t, _)) if t == "body"));
  }

  #[test]
  fn quotation_body_paragraph_has_first_line_indent_kern() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_quote_source(&style, QuoteKind::Quotation);

    // Assert
    let (_, _, children) = body_vbox(&nodes);
    let LayoutNode::Kern { length } = &children[0] else {
      panic!("quotation の本体先頭は字下げ Kern であるべき: {children:?}");
    };
    assert!((length.to_pt() - style.quote.first_line_indent.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn quote_body_uses_quote_style_font_kind() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_quote_source(&style, QuoteKind::Quote);

    // Assert
    let (_, _, children) = body_vbox(&nodes);
    let body_kind = children.iter().find_map(|n| match n {
      LayoutNode::Text(t, s) if t == "body" => return Some(s.font_kind),
      _ => return None,
    });
    assert_eq!(body_kind, Some(style.quote.font_kind));
  }
}
