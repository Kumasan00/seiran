//! 定理ブロック（`resolve::ResolvedNode::Theorem`）の lowering

use super::{
  LoweringContext, LoweringState,
  layout_node::{LayoutNode, TextStyle},
  lower_nodes_inner,
  template::expand_template,
  with_label_anchor,
};
use crate::{
  config::TheoremStyle,
  model::{Align, FontKind, LabelId, Length, TheoremClass},
  resolve::{ResolvedInline, ResolvedNode},
};

/// 定理ブロックをレイアウトノードに変換する
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_theorem(
  ctx: &LoweringContext,
  class: TheoremClass,
  number: Option<&str>,
  title: Option<&str>,
  body: &[ResolvedNode],
  of: Option<&LabelId>,
  label: Option<&LabelId>,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let theorem_style = ctx.style.theorem(class);
  let pres = &theorem_style.style;

  let mut nodes = vec![
    LayoutNode::Vkern {
      length: pres.top_margin,
    },
    build_heading(ctx, theorem_style, number, title, of, state),
  ];

  // 定理本体では文書本文の字下げを引き継がない。
  let body_ctx = ctx.with_body_font_kind(pres.font_kind).with_first_line_indent(Length::pt(0.0));
  let mut body_nodes = lower_nodes_inner(&body_ctx, body, state);

  if let Some(qed_mark) = theorem_style.qed_mark.as_deref() {
    let qed_node = make_qed_node(qed_mark, ctx.default_font_size());
    if matches!(body.last(), Some(ResolvedNode::Paragraph(_))) {
      let insert_at = body_nodes.len().saturating_sub(1);
      body_nodes.insert(insert_at, qed_node);
    } else {
      body_nodes.push(qed_node);
    }
  }
  nodes.extend(body_nodes);

  nodes.push(LayoutNode::Vkern {
    length: pres.bottom_margin,
  });

  return with_label_anchor(label, nodes);
}

/// 定理見出し（独立行）の `VBox` を構築する
fn build_heading(
  ctx: &LoweringContext,
  theorem_style: &TheoremStyle,
  number: Option<&str>,
  title: Option<&str>,
  of: Option<&LabelId>,
  state: &mut LoweringState,
) -> LayoutNode {
  let pres = &theorem_style.style;
  let base_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: pres.heading_font_kind,
    color: None,
  };

  let raw_template = match (of.is_some(), title.is_some()) {
    (true, true) => &pres.heading_with_of_and_title,
    (true, false) => &pres.heading_with_of,
    (false, true) => &pres.heading_with_title,
    (false, false) => &pres.heading_format,
  };
  let template = raw_template.replace("{display_name}", &theorem_style.display_name);

  let title_inlines: Vec<ResolvedInline> = title.map(|t| vec![ResolvedInline::Text(t.to_string())]).unwrap_or_default();

  let children = expand_template(ctx, &template, number.unwrap_or(""), &title_inlines, of, base_style, state);

  return LayoutNode::VBox {
    children,
    margin_bottom: Length::pt(0.0),
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    align: Align::Left,
  };
}

/// QED マークの右寄せノードを作る（既定サイズ・数式フォント）
fn make_qed_node(qed_mark: &str, font_size: Length) -> LayoutNode {
  let qed_style = TextStyle {
    font_size,
    font_kind: FontKind::Math,
    color: None,
  };
  return LayoutNode::FlushRight(vec![LayoutNode::Text(qed_mark.to_string(), qed_style)]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{super::test_support, *};
  use crate::{
    config::Style as ReadStyle,
    resolve::{self, CounterKind, CounterValue, ResolvedDocument},
  };

  /// テキスト 1 段落の本体を作るヘルパ
  fn paragraph(text: &str) -> ResolvedNode {
    return ResolvedNode::Paragraph(vec![ResolvedInline::Text(text.to_string())]);
  }

  /// `thm:p`（Theorem 1）を登録した解決済みドキュメントを作るヘルパ
  fn document_with_theorem() -> ResolvedDocument {
    return test_support::document(&[(
      "thm:p",
      CounterValue {
        kind: CounterKind::Theorem(TheoremClass::Theorem),
        parts: vec![1],
      },
    )]);
  }

  /// テスト用に `LoweringState` を構築して `lower_theorem` を呼ぶヘルパ
  fn lower_theorem_default(
    ctx: &LoweringContext,
    class: TheoremClass,
    number: Option<&str>,
    title: Option<&str>,
    body: &[ResolvedNode],
    label: Option<&LabelId>,
  ) -> Vec<LayoutNode> {
    let document = test_support::document(&[]);
    return lower_theorem(ctx, class, number, title, body, None, label, &mut LoweringState::new(&document));
  }

  /// `nodes` の最初の `VBox`（= 見出し）の子から先頭 `Text`（文字列・スタイル）を取り出す
  fn first_heading_text(nodes: &[LayoutNode]) -> (String, TextStyle) {
    let children = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children),
        _ => return None,
      })
      .expect("見出し VBox があるはず");
    return match &children[0] {
      LayoutNode::Text(t, s) => (t.clone(), *s),
      other => panic!("見出し先頭は Text であるべき: {other:?}"),
    };
  }

  #[test]
  fn theorem_renders_block_heading_and_italic_body() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Theorem, Some("1"), None, &[paragraph("body")], None);

    // Assert
    let (heading, heading_style) = first_heading_text(&nodes);
    assert_eq!(heading, "Theorem 1");
    assert_eq!(heading_style.font_kind, FontKind::SerifBold);
    let body = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "body" => return Some(*s),
        _ => return None,
      })
      .expect("本体 Text があるはず");
    assert_eq!(body.font_kind, FontKind::SerifItalic);
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は top_margin Vkern: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は bottom_margin Vkern: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::FlushRight(_))), "theorem に QED は出ない: {nodes:?}");
  }

  #[test]
  fn theorem_with_title_uses_heading_with_title_template() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes =
      lower_theorem_default(&ctx, TheoremClass::Theorem, Some("2"), Some("Pythagoras"), &[paragraph("x")], None);

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Theorem 2 (Pythagoras)");
  }

  #[test]
  fn proof_has_unnumbered_heading_roman_body_and_qed() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("qed")], None);

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof");
    let body = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "qed" => return Some(*s),
        _ => return None,
      })
      .expect("本体 Text があるはず");
    assert_eq!(body.font_kind, FontKind::Serif, "証明本体はローマン");
    let qed_count = nodes.iter().filter(|n| matches!(n, LayoutNode::FlushRight(_))).count();
    assert_eq!(qed_count, 1, "QED が 1 つ: {nodes:?}");
  }

  #[test]
  fn proof_qed_sits_in_last_paragraph_before_trailing_vkern() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("last")], None);

    // Assert
    let qed_idx = nodes.iter().position(|n| matches!(n, LayoutNode::FlushRight(_))).expect("QED があるはず");
    assert!(matches!(nodes.get(qed_idx + 1), Some(LayoutNode::Vkern { .. })), "QED の直後は Vkern: {nodes:?}");
    let text_idx = nodes.iter().position(|n| matches!(n, LayoutNode::Text(t, _) if t == "last")).unwrap();
    assert!(text_idx < qed_idx, "本体テキストは QED より前: {nodes:?}");
  }

  #[test]
  fn proof_qed_on_own_line_when_body_ends_with_non_paragraph() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let list = ResolvedNode::List {
      ordered: false,
      items: vec![resolve::ResolvedListItem {
        content: vec![paragraph("item")],
        marker: None,
        item_gap: None,
      }],
      start: None,
      item_gap: None,
    };

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[list], None);

    // Assert
    let qed_idx = nodes.iter().position(|n| matches!(n, LayoutNode::FlushRight(_))).expect("QED があるはず");
    assert_eq!(qed_idx, nodes.len() - 2, "QED は bottom_margin Vkern の直前: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })));
  }

  /// 見出し `VBox` 内の全 `Text`（`Link` に包まれた解決済み `\ref` も含む）を連結する
  fn heading_plain_text(nodes: &[LayoutNode]) -> String {
    let children = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children),
        _ => return None,
      })
      .expect("見出し VBox があるはず");
    return children.iter().map(flatten_text).collect();
  }

  fn flatten_text(node: &LayoutNode) -> String {
    return match node {
      LayoutNode::Text(t, _) => t.clone(),
      LayoutNode::Link { children, .. } => children.iter().map(flatten_text).collect(),
      _ => String::new(),
    };
  }

  #[test]
  fn proof_with_of_renders_proof_of_target_heading() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let document = document_with_theorem();

    // Act
    let nodes = lower_theorem(
      &ctx,
      TheoremClass::Proof,
      None,
      None,
      &[paragraph("x")],
      Some(&LabelId::new("thm:p")),
      None,
      &mut LoweringState::new(&document),
    );

    // Assert
    assert_eq!(heading_plain_text(&nodes), "Proof of Theorem 1");
  }

  #[test]
  fn proof_without_of_keeps_plain_proof_heading() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("x")], None);

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof");
  }

  #[test]
  fn proof_with_of_and_title_combines_both() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let document = document_with_theorem();

    // Act
    let nodes = lower_theorem(
      &ctx,
      TheoremClass::Proof,
      None,
      Some("sketch"),
      &[paragraph("x")],
      Some(&LabelId::new("thm:p")),
      None,
      &mut LoweringState::new(&document),
    );

    // Assert
    assert_eq!(heading_plain_text(&nodes), "Proof of Theorem 1 (sketch)");
  }

  #[test]
  fn proof_with_title_only_ignores_of_templates() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, Some("sketch"), &[paragraph("x")], None);

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof (sketch)");
  }

  #[test]
  fn theorem_with_label_prepends_anchor() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let label = LabelId::new("thm:x");
    let nodes = lower_theorem_default(&ctx, TheoremClass::Theorem, Some("1"), None, &[paragraph("b")], Some(&label));

    // Assert
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Anchor(crate::model::AnchorMark::Label(l))) if l.as_str() == "thm:x"),
      "先頭は Label アンカー: {nodes:?}"
    );
  }
}
