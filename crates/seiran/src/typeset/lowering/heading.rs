//! 見出し（`resolve::ResolvedNode::Heading`）の lowering

use model::{AnchorMark, HeadingKey, HeadingLevel, LabelId};

use super::{
  LoweringContext, LoweringState,
  layout_node::{LayoutNode, TextStyle},
  template::expand_template,
};
use crate::resolve::ResolvedInline;

/// 見出しをレイアウトノードに変換する
pub(super) fn lower_heading(
  ctx: &LoweringContext,
  level: HeadingLevel,
  number: &str,
  title: &[ResolvedInline],
  label: Option<LabelId>,
  heading_index: usize,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let heading_style = ctx.style.heading(level);
  let style = TextStyle {
    font_size: heading_style.font_size,
    font_kind: heading_style.font_kind,
    color: None,
  };

  let children = expand_template(ctx, &heading_style.format, number, title, None, style, state);

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  // しおり・目次リンク・`\ref` の到達先アンカー。改ページ後に置くことで正しいページに解決される。
  // `key` は文書順インデックスから決まる暗黙キー（目次エントリの内部リンクと一致させる）。
  result.push(LayoutNode::Anchor(AnchorMark::Heading {
    key: HeadingKey::new(heading_index),
    label,
  }));

  result.push(LayoutNode::VBox {
    children,
    margin_bottom: heading_style.bottom_margin,
    indent: model::Length::pt(0.0),
    right_indent: model::Length::pt(0.0),
    align: model::Align::Left,
  });

  // 見出し直後の改ページ制御。強制改ページ（page_break_after）と keep-with-next は排他:
  // page_break_after の見出し（Part 等）は意図的にページを終えるため keep-with-next を課さない。
  // それ以外の見出しは直後のブロックとの分割を禁止し、見出しがページ末尾に孤立するのを防ぐ。
  if heading_style.page_break_after {
    result.push(LayoutNode::PageBreak);
  } else {
    result.push(LayoutNode::KeepWithNext);
  }

  return result;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{super::test_support, *};
  use crate::{
    config::{CounterName, Style as ReadStyle},
    resolve::{CounterKind, CounterValue},
  };

  /// テスト用に `LoweringState` を構築して `lower_heading` を呼ぶヘルパ
  fn lower_heading_default(
    ctx: &LoweringContext,
    level: HeadingLevel,
    number: &str,
    title: &[ResolvedInline],
    label: Option<LabelId>,
    heading_index: usize,
  ) -> Vec<LayoutNode> {
    let document = test_support::document(&[]);
    return lower_heading(ctx, level, number, title, label, heading_index, &mut LoweringState::new(&document));
  }

  /// `nodes` から見出し `VBox` の子要素列を取り出す
  fn heading_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children.as_slice()),
        _ => return None,
      })
      .expect("VBox が出力されるはず");
  }

  #[test]
  fn lower_heading_uses_style_template() {
    // Arrange
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::Text("Custom Title".to_string())];

    // Act
    let nodes = lower_heading_default(&ctx, HeadingLevel::Section, "4.7", &title, None, 0);

    // Assert
    let children = heading_children(&nodes);
    let text = match &children[0] {
      LayoutNode::Text(text, _) => text.clone(),
      other => panic!("Text ノードが期待されます: {other:?}"),
    };
    assert_eq!(text, "[4.7] Custom Title");
  }

  #[test]
  fn lower_heading_preserves_styled_title() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [
      ResolvedInline::Text("Intro ".to_string()),
      ResolvedInline::Styled {
        kind: model::FontKind::SerifItalic,
        children: vec![ResolvedInline::Text("Italic".to_string())],
      },
    ];

    // Act
    let nodes = lower_heading_default(&ctx, HeadingLevel::Section, "1.1", &title, None, 0);

    // Assert
    let children = heading_children(&nodes);
    let heading_size = style.heading(HeadingLevel::Section).font_size;
    let italic = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "Italic" => return Some(*s),
        _ => return None,
      })
      .expect("イタリック部分の Text があるはず");
    assert_eq!(italic.font_kind, model::FontKind::SerifItalic);
    assert_eq!(italic.font_size, heading_size, "フォントサイズは見出しスタイルを継承する");
  }

  #[test]
  fn lower_heading_emits_anchor_with_label() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::Text("Intro".to_string())];

    // Act
    let nodes = lower_heading_default(&ctx, HeadingLevel::Section, "1", &title, Some(LabelId::new("sec:intro")), 3);

    // Assert
    let anchor = nodes.iter().find_map(|n| match n {
      LayoutNode::Anchor(mark) => return Some(mark.clone()),
      _ => return None,
    });
    assert_eq!(
      anchor,
      Some(model::AnchorMark::Heading {
        key: model::HeadingKey::new(3),
        label: Some(model::LabelId::new("sec:intro")),
      })
    );
    let anchor_idx = nodes.iter().position(|n| matches!(n, LayoutNode::Anchor(_))).unwrap();
    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    assert!(anchor_idx < vbox_idx, "アンカーは VBox より前: {nodes:?}");
  }

  #[test]
  fn lower_heading_emits_keep_with_next_after_vbox() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::Text("Intro".to_string())];

    // Act
    let nodes = lower_heading_default(&ctx, HeadingLevel::Section, "1", &title, None, 0);

    // Assert
    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    let keep_idx = nodes.iter().position(|n| matches!(n, LayoutNode::KeepWithNext)).expect("KeepWithNext が出るはず");
    assert!(keep_idx > vbox_idx, "KeepWithNext は VBox の後に出る: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "改ページは出ない: {nodes:?}");
  }

  #[test]
  fn lower_heading_with_page_break_after_omits_keep_with_next() {
    // Arrange
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].page_break_after = true;
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::Text("Intro".to_string())];

    // Act
    let nodes = lower_heading_default(&ctx, HeadingLevel::Section, "1", &title, None, 0);

    // Assert
    assert!(nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "強制改ページが出るはず: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::KeepWithNext)), "KeepWithNext は出ない: {nodes:?}");
  }

  #[test]
  fn ref_in_heading_title_is_resolved_to_internal_link() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::Ref {
      target: LabelId::new("sec:other"),
      span: model::Span::DUMMY,
    }];
    let document = test_support::document(&[(
      "sec:other",
      CounterValue {
        kind: CounterKind::Counter(CounterName::Section),
        parts: vec![0, 2, 3],
      },
    )]);

    // Act
    let nodes = lower_heading(&ctx, HeadingLevel::Section, "1", &title, None, 0, &mut LoweringState::new(&document));

    // Assert
    let children = heading_children(&nodes);
    let link = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Link { target, children } => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, model::LinkTarget::Internal(model::AnchorId::Label(LabelId::new("sec:other"))));
    assert!(matches!(&link.1[0], LayoutNode::Text(t, _) if t == "Section 2.3"), "{:?}", link.1);
  }
}
