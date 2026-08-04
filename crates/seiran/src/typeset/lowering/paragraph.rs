//! 段落（`resolve::ResolvedNode::Paragraph`）の lowering

use super::{
  LoweringContext, LoweringState,
  inline::lower_inline,
  layout_node::{LayoutNode, TextStyle},
};
use crate::resolve::ResolvedInline;

/// 段落をレイアウトノードに変換する
pub(super) fn lower_paragraph(
  ctx: &LoweringContext,
  inlines: &[ResolvedInline],
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let default_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.body_font_kind,
    color: None,
  };

  let mut result = Vec::new();

  // `\noindent`（[`ResolvedInline::NoIndent`] マーカー）が段落にあれば字下げを抑止する。位置検証は
  // パーサ（`evaluate_children`）が段落先頭に限定済みなので、ここでは存在の有無だけを見る。
  let suppress_indent = inlines.iter().any(|inline| matches!(inline, ResolvedInline::NoIndent));

  // 段落先頭行の字下げ。先頭に水平カーンを置くと、貪欲法ブレーカが先頭行だけ右へずらして
  // 折り返し幅を狭める（2 行目以降には残らない）。0pt のとき・`\noindent` 指定時は何も足さない。
  if ctx.first_line_indent.to_pt() > 0.0 && !suppress_indent {
    result.push(LayoutNode::Kern {
      length: ctx.first_line_indent,
    });
  }

  for inline in inlines {
    if matches!(inline, ResolvedInline::NoIndent) {
      continue;
    }
    result.extend(lower_inline(ctx, inline, default_style, state));
  }

  result.push(LayoutNode::Vkern {
    length: ctx.style.text.paragraph_spacing,
  });

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

  /// テキスト 1 つだけの段落を lower するテストヘルパ
  fn lower_plain(ctx: &LoweringContext, inlines: &[ResolvedInline]) -> Vec<LayoutNode> {
    let document = test_support::document(&[]);
    return lower_paragraph(ctx, inlines, &mut LoweringState::new(&document));
  }

  #[test]
  fn paragraph_appends_single_trailing_vkern_with_paragraph_spacing() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [ResolvedInline::Text("hello".to_string())];

    // Act
    let nodes = lower_plain(&ctx, &inlines);

    // Assert
    let LayoutNode::Vkern { length } = nodes.last().expect("末尾要素") else {
      panic!("末尾は Vkern であるべき: {nodes:?}");
    };
    assert!((length.to_pt() - style.text.paragraph_spacing.to_pt()).abs() < f32::EPSILON);
    let vkern_count = nodes.iter().filter(|n| matches!(n, LayoutNode::Vkern { .. })).count();
    assert_eq!(vkern_count, 1, "段落末の Vkern は 1 つだけ: {nodes:?}");
  }

  #[test]
  fn paragraph_text_uses_default_style_from_core_text() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [ResolvedInline::Text("body".to_string())];

    // Act
    let nodes = lower_plain(&ctx, &inlines);

    // Assert
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("先頭は Text であるべき: {nodes:?}");
    };
    assert_eq!(text, "body");
    assert_eq!(text_style.font_kind, style.text.font_kind);
    assert_eq!(text_style.font_size, ctx.default_font_size());
  }

  #[test]
  fn paragraph_prepends_first_line_indent_kern_when_positive() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_first_line_indent(model::Length::pt(15.0));
    let inlines = [ResolvedInline::Text("body".to_string())];

    // Act
    let nodes = lower_plain(&ctx, &inlines);

    // Assert
    let LayoutNode::Kern { length } = &nodes[0] else {
      panic!("先頭は字下げ Kern であるべき: {nodes:?}");
    };
    assert!((length.to_pt() - 15.0).abs() < f32::EPSILON);
    assert!(matches!(&nodes[1], LayoutNode::Text(t, _) if t == "body"));
  }

  #[test]
  fn paragraph_noindent_marker_suppresses_indent_kern() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_first_line_indent(model::Length::pt(15.0));
    let inlines = [
      ResolvedInline::NoIndent,
      ResolvedInline::Text("body".to_string()),
    ];

    // Act
    let nodes = lower_plain(&ctx, &inlines);

    // Assert
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::Kern { .. })), "字下げ Kern は抑止される: {nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "body"), "先頭は本文 Text: {nodes:?}");
  }

  #[test]
  fn paragraph_omits_first_line_indent_kern_by_default() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [ResolvedInline::Text("body".to_string())];

    // Act
    let nodes = lower_plain(&ctx, &inlines);

    // Assert
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "body"), "先頭は本文 Text: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::Kern { .. })), "字下げ Kern は出ない: {nodes:?}");
  }

  #[test]
  fn paragraph_preserves_inline_order() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [
      ResolvedInline::Text("one".to_string()),
      ResolvedInline::Text("two".to_string()),
      ResolvedInline::Text("three".to_string()),
    ];

    // Act
    let nodes = lower_plain(&ctx, &inlines);

    // Assert
    let texts: Vec<&str> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => return Some(t.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(texts, vec!["one", "two", "three"]);
  }

  #[test]
  fn paragraph_ref_is_resolved_to_internal_link() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [ResolvedInline::Ref {
      target: model::LabelId::new("eq:one"),
      span: model::Span::DUMMY,
    }];
    let document = test_support::document(&[(
      "eq:one",
      CounterValue {
        kind: CounterKind::Counter(CounterName::Equation),
        parts: vec![0, 1, 1],
      },
    )]);

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("解決済み \\ref は Link になるはず: {nodes:?}");
    };
    assert_eq!(*target, model::LinkTarget::Internal(model::AnchorId::Label(model::LabelId::new("eq:one"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "(1.1)"), "{children:?}");
  }
}
