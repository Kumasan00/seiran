//! 定理ブロック（`DocNode::Theorem`）の lowering

use config::TheoremStyle;
use model::{Align, DocNode, FontKind, InlineNode, Length, TheoremClass};

use super::{
  LoweringContext, LoweringError, PendingHeading,
  counter::CounterRegistry,
  layout_node::{LayoutNode, TextStyle},
  lower_nodes_inner,
  template::expand_template,
  with_label_anchor,
};

/// 定理ブロックをレイアウトノードに変換する
///
/// # Errors
///
/// 見出しテンプレート・本体の lowering が返す [`LoweringError`]（未解決 `\ref` 等）を伝播する。
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_theorem(
  ctx: &LoweringContext,
  class: TheoremClass,
  number: Option<&str>,
  title: Option<&str>,
  body: &[DocNode],
  of: Option<(&str, model::Span)>,
  label: Option<&str>,
  registry: &mut CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let theorem_style = ctx.style.theorem(class);
  let pres = &theorem_style.style;

  let mut nodes = vec![
    LayoutNode::Vkern {
      length: pres.top_margin,
    },
    build_heading(ctx, theorem_style, number, title, of, registry)?,
  ];

  // 定理本体では文書本文の字下げを引き継がない。
  let body_ctx = ctx.with_body_font_kind(pres.font_kind).with_first_line_indent(Length::pt(0.0));
  let mut body_nodes = lower_nodes_inner(&body_ctx, body, registry, headings)?;

  if let Some(qed_mark) = theorem_style.qed_mark.as_deref() {
    let qed_node = make_qed_node(qed_mark, ctx.default_font_size());
    if matches!(body.last(), Some(DocNode::Paragraph(_))) {
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

  return Ok(with_label_anchor(label, nodes));
}

/// 定理見出し（独立行）の `VBox` を構築する
fn build_heading(
  ctx: &LoweringContext,
  theorem_style: &TheoremStyle,
  number: Option<&str>,
  title: Option<&str>,
  of: Option<(&str, model::Span)>,
  registry: &mut CounterRegistry,
) -> Result<LayoutNode, LoweringError> {
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

  let title_inlines: Vec<InlineNode> = title.map(|t| vec![InlineNode::Text(t.to_string())]).unwrap_or_default();

  let children = expand_template(ctx, &template, number.unwrap_or(""), &title_inlines, of, base_style, registry)?;

  return Ok(LayoutNode::VBox {
    children,
    margin_bottom: Length::pt(0.0),
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    align: Align::Left,
  });
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
mod tests {
  use config::Style as ReadStyle;
  use model::{DocNode, FontKind};

  use super::*;

  /// テキスト 1 段落の本体を作るヘルパ
  fn paragraph(text: &str) -> DocNode { return DocNode::Paragraph(vec![InlineNode::Text(text.to_string())]); }

  fn dummy_span() -> model::Span { return model::Span::DUMMY; }

  /// テスト用に新規 `CounterRegistry` / 見出し記録バッファを構築して `lower_theorem` を呼ぶヘルパ
  #[allow(clippy::too_many_arguments)]
  fn lower_theorem_default(
    ctx: &LoweringContext,
    class: TheoremClass,
    number: Option<&str>,
    title: Option<&str>,
    body: &[DocNode],
    of: Option<&str>,
    label: Option<&str>,
  ) -> Result<Vec<LayoutNode>, LoweringError> {
    let mut registry = CounterRegistry::from_style(ctx.style);
    let mut headings = Vec::new();
    let of = of.map(|l| return (l, dummy_span()));
    return lower_theorem(ctx, class, number, title, body, of, label, &mut registry, &mut headings);
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
    let nodes = lower_theorem_default(&ctx, TheoremClass::Theorem, Some("1"), None, &[paragraph("body")], None, None)
      .expect("失敗しないはず");

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
      lower_theorem_default(&ctx, TheoremClass::Theorem, Some("2"), Some("Pythagoras"), &[paragraph("x")], None, None)
        .expect("失敗しないはず");

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
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("qed")], None, None)
      .expect("失敗しないはず");

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
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("last")], None, None)
      .expect("失敗しないはず");

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
    let list = DocNode::List {
      ordered: false,
      items: vec![model::ListItem::new(vec![paragraph("item")])],
      start: None,
      item_gap: None,
    };

    // Act
    let nodes =
      lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[list], None, None).expect("失敗しないはず");

    // Assert
    let qed_idx = nodes.iter().position(|n| matches!(n, LayoutNode::FlushRight(_))).expect("QED があるはず");
    assert_eq!(qed_idx, nodes.len() - 2, "QED は bottom_margin Vkern の直前: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })));
  }

  /// 見出し `VBox` 内の全 `Text`（`Link` に包まれた解決済み `Ref` も含む）を連結する
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
    let mut registry = CounterRegistry::from_style(&style);
    registry
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        Some("thm:p"),
        dummy_span(),
        model::Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();
    let mut headings = Vec::new();

    // Act
    let mut nodes = lower_theorem(
      &ctx,
      TheoremClass::Proof,
      None,
      None,
      &[paragraph("x")],
      Some(("thm:p", dummy_span())),
      None,
      &mut registry,
      &mut headings,
    )
    .expect("失敗しないはず");
    super::super::resolve::resolve_refs(&mut nodes, &registry).expect("thm:p は登録済みなので解決できる");

    // Assert
    assert_eq!(heading_plain_text(&nodes), "Proof of Theorem 1");
  }

  #[test]
  fn proof_without_of_keeps_plain_proof_heading() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("x")], None, None)
      .expect("失敗しないはず");

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof");
  }

  #[test]
  fn proof_with_of_and_title_combines_both() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::from_style(&style);
    registry
      .increment_theorem_with_label(
        TheoremClass::Theorem,
        Some("thm:p"),
        dummy_span(),
        model::Origin::Source(model::SourceId::new(0)),
      )
      .unwrap();
    let mut headings = Vec::new();

    // Act
    let mut nodes = lower_theorem(
      &ctx,
      TheoremClass::Proof,
      None,
      Some("sketch"),
      &[paragraph("x")],
      Some(("thm:p", dummy_span())),
      None,
      &mut registry,
      &mut headings,
    )
    .expect("失敗しないはず");
    super::super::resolve::resolve_refs(&mut nodes, &registry).expect("thm:p は登録済みなので解決できる");

    // Assert
    assert_eq!(heading_plain_text(&nodes), "Proof of Theorem 1 (sketch)");
  }

  #[test]
  fn proof_with_title_only_ignores_of_templates() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, Some("sketch"), &[paragraph("x")], None, None)
      .expect("失敗しないはず");

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof (sketch)");
  }

  #[test]
  fn theorem_with_label_prepends_anchor() {
    // Arrange / Act
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes =
      lower_theorem_default(&ctx, TheoremClass::Theorem, Some("1"), None, &[paragraph("b")], None, Some("thm:x"))
        .expect("失敗しないはず");

    // Assert
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Anchor(model::AnchorMark::Label(l))) if l.as_str() == "thm:x"),
      "先頭は Label アンカー: {nodes:?}"
    );
  }
}
