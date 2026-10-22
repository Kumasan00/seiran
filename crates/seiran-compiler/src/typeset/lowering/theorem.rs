//! 定理ブロック（`document::HirNodeKind::Theorem`）の lowering

use crate::{
  document::{HirNodeKind, HirTheorem, NodeId, Typeface},
  length::Length,
  semantics::LabelId,
  style::{TheoremHeadingValues, TheoremStyle},
  typeset::lowering::{
    LoweringContext, LoweringState, counter,
    layout_node::{InlineNode, LayoutNode, ParagraphLayout, TextStyle, merge_adjacent_text},
    lower_nodes, with_label_anchors,
  },
};

/// 定理ブロックをレイアウトノードに変換する
pub(super) fn lower_theorem(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  theorem: &HirTheorem,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let number = state.counter_value(id).map(|value| return counter::format_counter_value(ctx.style, value));
  let of_target = theorem.of.as_ref().map(|target| return state.ref_target(target.id));
  let label = state.declared_label(id);

  let theorem_style = &ctx.style.theorems[theorem.class];
  let pres = &theorem_style.presentation;

  let mut nodes = vec![
    LayoutNode::Vkern {
      length: pres.top_margin,
    },
    build_heading(ctx, id, theorem_style, number.as_deref(), theorem.title.as_deref(), of_target, state),
  ];

  let body_ctx = ctx.with_body_typeface(pres.typeface).with_first_line_indent(Length::pt(0.0));
  let mut body_nodes = lower_nodes(&body_ctx, &theorem.body, state);

  if let Some(qed_mark) = theorem_style.qed_mark.as_deref() {
    let qed_node = make_qed_node(qed_mark, ctx.default_font_size());
    // 空リストは何も出力しない。末尾の本文段落には続け、空リストを挟んでも未完の水平アキ列には続ける。
    let last_emitting = theorem
      .body
      .iter()
      .rev()
      .find(|node| return !matches!(&node.kind, HirNodeKind::List(list) if list.items.is_empty()));
    let joins_body = matches!(theorem.body.last().map(|node| return &node.kind), Some(HirNodeKind::Paragraph(_)))
      || matches!(last_emitting.map(|node| return &node.kind), Some(HirNodeKind::Space(_)));
    if joins_body {
      let Some(LayoutNode::Paragraph(paragraph)) = body_nodes.last_mut() else {
        unreachable!("lower_nodes は末尾の本文段落と水平アキ列を Paragraph として出力する")
      };
      paragraph.content.push(qed_node);
    } else {
      body_nodes.push(LayoutNode::Paragraph(ParagraphLayout {
        origin: Some(id),
        content: vec![qed_node],
        spacing_after: Length::ZERO,
      }));
    }
  }
  nodes.extend(body_nodes);

  nodes.push(LayoutNode::Vkern {
    length: pres.bottom_margin,
  });

  return with_label_anchors(label, nodes);
}

/// 定理見出し（独立行）の `VBox` を構築する
fn build_heading(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  theorem_style: &TheoremStyle,
  number: Option<&str>,
  title: Option<&str>,
  of: Option<&LabelId>,
  state: &LoweringState<'_>,
) -> LayoutNode {
  let pres = &theorem_style.presentation;
  let base_style = TextStyle {
    font_size: ctx.default_font_size(),
    typeface: pres.heading_typeface,
    color: None,
    script_level: None,
    math_operator: None,
  };

  let template = match (of.is_some(), title.is_some()) {
    (true, true) => &pres.heading_with_of_and_title,
    (true, false) => &pres.heading_with_of,
    (false, true) => &pres.heading_with_title,
    (false, false) => &pres.heading_format,
  };

  // 副作用のない生成なので、遅延させても結果は変わらない。
  let make_title = || {
    return title.map(|t| return vec![InlineNode::Text(t.to_string(), base_style)]).unwrap_or_default();
  };
  let of_display = of.map(|target| return state.ref_display(ctx.style, target));

  let values = TheoremHeadingValues {
    display_name: &theorem_style.display_name,
    number: number.unwrap_or(""),
    of: of_display.as_deref(),
  };
  let children = template.expand(values, make_title, |literal| {
    return InlineNode::Text(literal.to_string(), base_style);
  });
  let children = vec![LayoutNode::Paragraph(ParagraphLayout {
    origin: Some(id),
    content: merge_adjacent_text(children),
    spacing_after: Length::ZERO,
  })];

  return LayoutNode::VBox {
    children,
    margin_bottom: Length::pt(0.0),
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    alignment: None,
  };
}

/// QED マークの右寄せノードを作る（既定サイズ・数式フォント）
fn make_qed_node(qed_mark: &str, font_size: Length) -> InlineNode {
  let qed_style = TextStyle {
    font_size,
    typeface: Typeface::Math,
    color: None,
    script_level: None,
    math_operator: None,
  };
  return InlineNode::FlushRight(qed_mark.to_string(), qed_style);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    style::Style as ReadStyle,
    typeset::{
      boxes::AnchorId,
      lowering::test_support::{analyzed, inlines, lower, paragraph_layout},
    },
  };

  /// `.sei` ソースを lower してレイアウトノード列を返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  /// `nodes` の最初の `VBox`（= 定理見出し）の子から先頭 `Text`（文字列・スタイル）を取り出す
  fn first_heading_text(nodes: &[LayoutNode]) -> (String, TextStyle) {
    let children = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children),
        _ => return None,
      })
      .expect("見出し VBox があるはず");
    return match &paragraph_layout(&children[0]).content[0] {
      InlineNode::Text(t, s) => (t.clone(), *s),
      other => panic!("見出し先頭は Text であるべき: {other:?}"),
    };
  }

  /// 最後の見出し `VBox` 内の全 `Text`（`Link` に包まれた解決済み `\ref` も含む）を連結する
  fn last_heading_plain_text(nodes: &[LayoutNode]) -> String {
    let children = nodes
      .iter()
      .rev()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children),
        _ => return None,
      })
      .expect("見出し VBox があるはず");
    return children.iter().map(flatten_text).collect();
  }

  /// レイアウトノードから表示テキストだけを取り出す
  fn flatten_text(node: &LayoutNode) -> String {
    return match node {
      LayoutNode::Paragraph(paragraph) => paragraph.content.iter().map(flatten_text_inline).collect(),
      _ => String::new(),
    };
  }

  /// [`flatten_text`] のインライン列側（`Link` の子は `InlineNode`）
  fn flatten_text_inline(node: &InlineNode) -> String {
    return match node {
      InlineNode::Text(t, _) => t.clone(),
      InlineNode::Link { children, .. } => children.iter().map(flatten_text_inline).collect(),
      _ => String::new(),
    };
  }

  #[test]
  fn theorem_renders_block_heading_and_italic_body() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{theorem}\nbody\n\\end{theorem}\n");

    let (heading, heading_style) = first_heading_text(&nodes);
    assert_eq!(heading, "Theorem 1");
    assert_eq!(heading_style.typeface, Typeface::SerifBold);
    let body = inlines(&nodes)
      .find_map(|n| match n {
        InlineNode::Text(t, s) if t == "body" => return Some(*s),
        _ => return None,
      })
      .expect("本体 Text があるはず");
    assert_eq!(body.typeface, Typeface::SerifItalic);
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は top_margin Vkern: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は bottom_margin Vkern: {nodes:?}");
    assert!(
      !inlines(&nodes).any(|n| matches!(n, InlineNode::FlushRight(..))),
      "theorem に QED は出ない: {nodes:?}"
    );
  }

  #[test]
  fn theorem_with_title_uses_heading_with_title_template() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{theorem}[title=Pythagoras]\nx\n\\end{theorem}\n");

    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Theorem 1 (Pythagoras)");
  }

  #[test]
  fn proof_has_unnumbered_heading_roman_body_and_qed() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{proof}\nqed\n\\end{proof}\n");

    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof");
    let body = inlines(&nodes)
      .find_map(|n| match n {
        InlineNode::Text(t, s) if t == "qed" => return Some(*s),
        _ => return None,
      })
      .expect("本体 Text があるはず");
    assert_eq!(body.typeface, Typeface::Serif, "証明本体はローマン");
    let qed_count = inlines(&nodes).filter(|n| matches!(n, InlineNode::FlushRight(..))).count();
    assert_eq!(qed_count, 1, "QED が 1 つ: {nodes:?}");
  }

  #[test]
  fn proof_qed_sits_in_last_paragraph_before_its_spacing() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{proof}\nlast\n\\end{proof}\n");

    let paragraph = paragraph_layout(&nodes[nodes.len() - 2]);
    assert!(
      matches!(paragraph.content.as_slice(), [InlineNode::Text(text, _), InlineNode::FlushRight(..)] if text == "last")
    );
    assert_eq!(paragraph.spacing_after, style.text.paragraph_spacing);
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })));
  }

  #[test]
  fn proof_qed_on_own_line_when_body_ends_with_non_paragraph() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{proof}\n\\begin{itemize}\n\\item{item}\n\\end{itemize}\n\\end{proof}\n");

    let paragraph = paragraph_layout(&nodes[nodes.len() - 2]);
    assert!(matches!(paragraph.content.as_slice(), [InlineNode::FlushRight(..)]));
    assert_eq!(paragraph.spacing_after, Length::ZERO);
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })));
  }

  #[test]
  fn empty_list_after_body_paragraph_keeps_qed_on_its_own_line() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, r"\begin{proof}body\begin{itemize}\end{itemize}\end{proof}");

    let body = paragraph_layout(&nodes[nodes.len() - 3]);
    let qed = paragraph_layout(&nodes[nodes.len() - 2]);
    assert!(matches!(body.content.as_slice(), [InlineNode::Text(text, _)] if text == "body"));
    assert_eq!(body.spacing_after, style.text.paragraph_spacing);
    assert!(matches!(qed.content.as_slice(), [InlineNode::FlushRight(..)]));
    assert_eq!(qed.spacing_after, Length::ZERO);
  }

  #[test]
  fn trailing_horizontal_space_shares_the_qed_line_through_an_empty_list() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, r"\begin{proof}\space{5pt}\begin{itemize}\end{itemize}\end{proof}");

    let paragraph = paragraph_layout(&nodes[nodes.len() - 2]);
    assert!(
      matches!(paragraph.content.as_slice(), [InlineNode::Kern { length }, InlineNode::FlushRight(..)] if *length == Length::pt(5.0))
    );
    assert_eq!(paragraph.spacing_after, Length::ZERO);
  }

  #[test]
  fn proof_with_of_renders_proof_of_target_heading() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{theorem}[label=thm:p]\np\n\\end{theorem}\n\n\\begin{proof}[of=thm:p]\nx\n\\end{proof}\n",
    );

    assert_eq!(last_heading_plain_text(&nodes), "Proof of Theorem 1");
  }

  #[test]
  fn proof_with_of_and_title_combines_both() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{theorem}[label=thm:p]\np\n\\end{theorem}\n\n\
       \\begin{proof}[of=thm:p, title=sketch]\nx\n\\end{proof}\n",
    );

    assert_eq!(last_heading_plain_text(&nodes), "Proof of Theorem 1 (sketch)");
  }

  #[test]
  fn theorem_with_label_prepends_anchor() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{theorem}[label=thm:x]\nb\n\\end{theorem}\n");

    assert!(
      matches!(nodes.first(), Some(LayoutNode::Anchor(AnchorId::Label(l))) if l.as_str() == "thm:x"),
      "先頭は Label アンカー: {nodes:?}"
    );
  }
}
