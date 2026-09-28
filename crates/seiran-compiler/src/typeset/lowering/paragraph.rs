//! 段落（`document::HirNodeKind::Paragraph`）の lowering

use crate::{
  document::{HirInline, HirInlineKind},
  typeset::lowering::{
    LoweringContext, LoweringState,
    inline::lower_inline,
    layout_node::{InlineNode, LayoutNode, TextStyle},
  },
};

/// 本文段落の既定テキストスタイルを返す
pub(super) fn body_text_style(ctx: &LoweringContext<'_>) -> TextStyle {
  return TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.body_font_kind,
    color: None,
  };
}

/// 段落の箱組み（先頭行の字下げカーン + 内容 + 段落間アキ）
pub(super) fn assemble_paragraph(
  ctx: &LoweringContext<'_>,
  content: Vec<InlineNode>,
  suppress_indent: bool,
) -> Vec<LayoutNode> {
  let mut result = Vec::with_capacity(content.len() + 2);

  // 段落先頭行の字下げ。先頭に水平カーンを置くと、行分割が先頭行だけ右へずらして
  // 折り返し幅を狭める（2 行目以降には残らない）。0pt のとき・`\noindent` 指定時は何も足さない。
  if ctx.first_line_indent.to_pt() > 0.0 && !suppress_indent {
    result.push(LayoutNode::Inline(InlineNode::Kern {
      length: ctx.first_line_indent,
    }));
  }

  result.extend(content.into_iter().map(LayoutNode::from));

  result.push(LayoutNode::Vkern {
    length: ctx.style.text.paragraph_spacing,
  });

  return result;
}

/// 段落をレイアウトノードに変換する
pub(super) fn lower_paragraph(
  ctx: &LoweringContext<'_>,
  inlines: &[HirInline],
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let default_style = body_text_style(ctx);

  // `\noindent`（[`HirInlineKind::NoIndent`] マーカー）が段落にあれば字下げを抑止する。位置検証は
  // 評価器（`evaluate_children`）が段落先頭に限定済みなので、ここでは存在の有無だけを見る。
  let suppress_indent = inlines.iter().any(|inline| matches!(inline.kind, HirInlineKind::NoIndent));

  let mut content: Vec<InlineNode> = Vec::new();
  for inline in inlines {
    if matches!(inline.kind, HirInlineKind::NoIndent) {
      continue;
    }
    content.extend(lower_inline(ctx, inline, default_style, state));
  }

  return assemble_paragraph(ctx, content, suppress_indent);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    length::Length,
    semantics::LabelId,
    style::Style as ReadStyle,
    typeset::{
      boxes::{AnchorId, LinkTarget},
      lowering::test_support::{analyzed, lower},
    },
  };

  /// 段落 1 つの `.sei` ソースを lower するテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  #[test]
  fn paragraph_appends_single_trailing_vkern_with_paragraph_spacing() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "hello\n");

    let LayoutNode::Vkern { length } = nodes.last().expect("末尾要素") else {
      panic!("末尾は Vkern であるべき: {nodes:?}");
    };
    assert!((length.to_pt() - style.text.paragraph_spacing.to_pt()).abs() < f32::EPSILON);
    let vkern_count = nodes.iter().filter(|n| matches!(n, LayoutNode::Vkern { .. })).count();
    assert_eq!(vkern_count, 1, "段落末の Vkern は 1 つだけ: {nodes:?}");
  }

  #[test]
  fn paragraph_text_uses_default_style_from_core_text() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "body\n");

    let LayoutNode::Inline(InlineNode::Text(text, text_style)) = &nodes[0] else {
      panic!("先頭は Text であるべき: {nodes:?}");
    };
    assert_eq!(text, "body");
    assert_eq!(text_style.font_kind, style.text.font_kind);
    assert_eq!(text_style.font_size, style.text.font_size);
  }

  #[test]
  fn paragraph_prepends_first_line_indent_kern_when_positive() {
    let mut style = ReadStyle::default();
    style.text.first_line_indent = Length::pt(15.0);

    let nodes = lower_source(&style, "body\n");

    let LayoutNode::Inline(InlineNode::Kern { length }) = &nodes[0] else {
      panic!("先頭は字下げ Kern であるべき: {nodes:?}");
    };
    assert!((length.to_pt() - 15.0).abs() < f32::EPSILON);
    assert!(matches!(&nodes[1], LayoutNode::Inline(InlineNode::Text(t, _)) if t == "body"));
  }

  #[test]
  fn paragraph_noindent_marker_suppresses_indent_kern() {
    let mut style = ReadStyle::default();
    style.text.first_line_indent = Length::pt(15.0);

    let nodes = lower_source(&style, "\\noindent body\n");

    assert!(
      !nodes.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Kern { .. }))),
      "字下げ Kern は抑止される: {nodes:?}"
    );
    assert!(
      matches!(&nodes[0], LayoutNode::Inline(InlineNode::Text(t, _)) if t == "body"),
      "先頭は本文 Text: {nodes:?}"
    );
  }

  #[test]
  fn paragraph_omits_first_line_indent_kern_by_default() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "body\n");

    assert!(
      matches!(&nodes[0], LayoutNode::Inline(InlineNode::Text(t, _)) if t == "body"),
      "先頭は本文 Text: {nodes:?}"
    );
    assert!(
      !nodes.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Kern { .. }))),
      "字下げ Kern は出ない: {nodes:?}"
    );
  }

  #[test]
  fn paragraph_preserves_inline_order() {
    let style = ReadStyle::default();

    // 書体切り替えを挟んで、インラインが Text へ落ちる順序を見る
    let nodes = lower_source(&style, "\\italic{one}\\bold{two}\\mono{three}\n");

    let texts: Vec<&str> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Inline(InlineNode::Text(t, _)) => return Some(t.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(texts, vec!["one", "two", "three"]);
  }

  #[test]
  fn paragraph_ref_is_resolved_to_internal_link() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\chapter[label=ch:one]{Intro}\n\n\\ref{ch:one}\n");

    // 段落は解決済み `\ref` をそのままリンクとして通す
    let link = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Inline(InlineNode::Link { target, children }) => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, LinkTarget::Internal(AnchorId::Label(LabelId::new("ch:one"))));
    assert!(matches!(&link.1[0], InlineNode::Text(t, _) if t == "Chapter 1"), "{:?}", link.1);
  }
}
