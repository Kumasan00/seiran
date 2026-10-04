//! 寄せ環境（`document::HirNodeKind::Flush`）の lowering

use crate::{
  document::HirFlush,
  length::Length,
  typeset::lowering::{LoweringContext, LoweringState, layout_node::LayoutNode, lower_nodes},
};

/// 寄せ環境を、本体が外側から継ぐ揃えだけを環境の向きに置き換える `VBox` に変換する
///
/// 揃えを自分で決める子（レベルに揃えを指定した見出し・図表・コードブロックの `Some` な `VBox`、数式ブロック・
/// 表のセルの `Align`）はこの値を読まないので変わらない。字下げ・書体・`first_line_indent`・上下の余白は足さない。
pub(super) fn lower_flush(
  ctx: &LoweringContext<'_>,
  flush: &HirFlush,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  return vec![LayoutNode::VBox {
    children: lower_nodes(ctx, &flush.body, state),
    margin_bottom: Length::pt(0.0),
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    alignment: Some(flush.alignment),
  }];
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    document::TextAlignment,
    style::Style as ReadStyle,
    typeset::lowering::{
      layout_node::InlineNode,
      test_support::{analyzed, lower},
    },
  };

  /// `source` を lower し、トップレベルがただ 1 つの寄せ環境の `VBox` であることを確かめて、その揃えと子を返す
  fn lower_single_flush(style: &ReadStyle, source: &str) -> (Option<TextAlignment>, Vec<LayoutNode>) {
    let nodes = lower(style, &analyzed(source));
    let [
      LayoutNode::VBox {
        children,
        margin_bottom,
        indent,
        right_indent,
        alignment,
      },
    ] = nodes.as_slice()
    else {
      panic!("寄せ環境は VBox 1 つだけを出すはず: {nodes:?}");
    };
    assert_eq!(*margin_bottom, Length::pt(0.0), "寄せ環境は下の余白を足さない");
    assert_eq!(*indent, Length::pt(0.0), "寄せ環境は左の字下げを足さない");
    assert_eq!(*right_indent, Length::pt(0.0), "寄せ環境は右の字下げを足さない");
    return (*alignment, children.clone());
  }

  /// `children` に現れる `VBox` の揃えを出現順に集める（入れ子の中までは降りない）
  fn child_vbox_alignments(children: &[LayoutNode]) -> Vec<Option<TextAlignment>> {
    return children
      .iter()
      .filter_map(|node| match node {
        LayoutNode::VBox { alignment, .. } => return Some(*alignment),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn each_environment_replaces_alignment_with_its_direction() {
    for (name, expected) in [
      ("flushleft", TextAlignment::Left),
      ("center", TextAlignment::Center),
      ("flushright", TextAlignment::Right),
    ] {
      let (alignment, children) =
        lower_single_flush(&ReadStyle::default(), &format!("\\begin{{{name}}}\nbody\n\\end{{{name}}}\n"));

      assert_eq!(alignment, Some(expected), "{name}");
      assert!(
        children.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Text(t, _)) if t == "body")),
        "本体の段落が子に入るはず（{name}）: {children:?}"
      );
    }
  }

  #[test]
  fn body_keeps_text_first_line_indent() {
    let mut style = ReadStyle::default();
    style.text.first_line_indent = Length::pt(10.0);

    let (_, children) = lower_single_flush(&style, "\\begin{center}\nbody\n\\end{center}\n");

    let Some(LayoutNode::Inline(InlineNode::Kern { length })) = children.first() else {
      panic!("本体の段落は [text].first_line_indent の字下げで始まるはず: {children:?}");
    };
    assert_eq!(*length, Length::pt(10.0));
  }

  #[test]
  fn heading_inherits_direction_unless_its_level_specifies_alignment() {
    let source = "\\begin{flushright}\n\\subsection{見出し}\n\\end{flushright}\n";
    let mut specified = ReadStyle::default();
    specified.heading.subsection.alignment = Some(TextAlignment::Left);

    let (_, inherited_children) = lower_single_flush(&ReadStyle::default(), source);
    let (_, specified_children) = lower_single_flush(&specified, source);

    assert_eq!(child_vbox_alignments(&inherited_children), vec![None], "未指定の見出しは環境の向きを継ぐ");
    assert_eq!(
      child_vbox_alignments(&specified_children),
      vec![Some(TextAlignment::Left)],
      "指定した見出しはそのまま"
    );
  }

  #[test]
  fn nested_flush_and_code_keep_their_own_alignment() {
    let source =
      "\\begin{flushright}\n\\begin{center}\nx\n\\end{center}\n\n\\begin{code}\ny\n\\end{code}\n\\end{flushright}\n";

    let (alignment, children) = lower_single_flush(&ReadStyle::default(), source);

    assert_eq!(alignment, Some(TextAlignment::Right));
    assert_eq!(
      child_vbox_alignments(&children),
      vec![Some(TextAlignment::Center), Some(TextAlignment::Left)],
      "内側の寄せ環境は自分の向き、コードブロックは左固定"
    );
  }
}
