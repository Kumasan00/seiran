//! 引用ブロック（`document::HirNodeKind::Quote`）の lowering

use crate::{
  document::HirQuote,
  length::Length,
  typeset::{
    boxes::Align,
    lowering::{LoweringContext, LoweringState, layout_node::LayoutNode, lower_nodes},
  },
};

/// 引用ブロックをレイアウトノードに変換する
pub(super) fn lower_quote(
  ctx: &LoweringContext<'_>,
  quote: &HirQuote,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let style = &ctx.style.quote;

  let first_line_indent = if quote.kind.indents_first_line() {
    style.first_line_indent
  } else {
    Length::pt(0.0)
  };
  let body_ctx = ctx.with_body_typeface(style.typeface).with_first_line_indent(first_line_indent);
  let children = lower_nodes(&body_ctx, &quote.body, state);

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
mod tests {
  use super::*;
  use crate::{
    document::Typeface,
    style::Style as ReadStyle,
    typeset::lowering::{
      layout_node::InlineNode,
      test_support::{analyzed, lower},
    },
  };

  /// 環境 `name`（`quote` / `quotation`）1 つだけの `.sei` ソースを lower するヘルパ
  fn lower_quote_source(style: &ReadStyle, name: &str) -> Vec<LayoutNode> {
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
    let style = ReadStyle::default();

    let nodes = lower_quote_source(&style, "quote");

    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は top_margin Vkern: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は bottom_margin Vkern: {nodes:?}");
    let (indent, right_indent, _) = body_vbox(&nodes);
    assert!((indent.to_pt() - style.quote.indent.to_pt()).abs() < f32::EPSILON);
    assert!((right_indent.to_pt() - style.quote.indent.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn quote_body_paragraph_has_no_first_line_indent_kern() {
    let style = ReadStyle::default();

    let nodes = lower_quote_source(&style, "quote");

    let (_, _, children) = body_vbox(&nodes);
    assert!(
      !children.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Kern { .. }))),
      "quote に字下げ Kern は出ない: {children:?}"
    );
    assert!(matches!(children.first(), Some(LayoutNode::Inline(InlineNode::Text(t, _))) if t == "body"));
  }

  #[test]
  fn quotation_body_paragraph_has_first_line_indent_kern() {
    let style = ReadStyle::default();

    let nodes = lower_quote_source(&style, "quotation");

    let (_, _, children) = body_vbox(&nodes);
    let LayoutNode::Inline(InlineNode::Kern { length }) = &children[0] else {
      panic!("quotation の本体先頭は字下げ Kern であるべき: {children:?}");
    };
    assert!((length.to_pt() - style.quote.first_line_indent.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn quote_body_uses_quote_style_typeface() {
    // [text].typeface の既定（Serif）と区別できる値にする
    let mut style = ReadStyle::default();
    style.quote.typeface = Typeface::SansSerif;

    let nodes = lower_quote_source(&style, "quote");

    let (_, _, children) = body_vbox(&nodes);
    let body_kind = children.iter().find_map(|n| match n {
      LayoutNode::Inline(InlineNode::Text(t, s)) if t == "body" => return Some(s.typeface),
      _ => return None,
    });
    assert_eq!(body_kind, Some(Typeface::SansSerif), "引用本文は [quote] の書体に従う");
  }
}
