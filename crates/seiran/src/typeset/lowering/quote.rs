//! 引用ブロック（`resolve::ResolvedNode::Quote`）の lowering

use model::{Align, Length, QuoteKind};
use resolve::ResolvedNode;

use super::{LoweringContext, LoweringState, layout_node::LayoutNode, lower_nodes_inner};

/// 引用ブロックをレイアウトノードに変換する
pub(super) fn lower_quote(
  ctx: &LoweringContext,
  kind: QuoteKind,
  body: &[ResolvedNode],
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
  use config::Style as ReadStyle;
  use model::QuoteKind;
  use resolve::ResolvedInline;

  use super::{super::test_support, *};

  /// テキスト 1 段落の本体を作るヘルパ
  fn paragraph(text: &str) -> ResolvedNode {
    return ResolvedNode::Paragraph(vec![ResolvedInline::Text(text.to_string())]);
  }

  /// テスト用に `LoweringState` を構築して `lower_quote` を呼ぶヘルパ
  fn lower_quote_default(ctx: &LoweringContext, kind: QuoteKind, body: &[ResolvedNode]) -> Vec<LayoutNode> {
    let document = test_support::document(&[]);
    return lower_quote(ctx, kind, body, &mut LoweringState::new(&document));
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
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quote, &[paragraph("body")]);

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
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quote, &[paragraph("body")]);

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
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quotation, &[paragraph("body")]);

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
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quote, &[paragraph("body")]);

    // Assert
    let (_, _, children) = body_vbox(&nodes);
    let body_kind = children.iter().find_map(|n| match n {
      LayoutNode::Text(t, s) if t == "body" => return Some(s.font_kind),
      _ => return None,
    });
    assert_eq!(body_kind, Some(style.quote.font_kind));
  }
}
