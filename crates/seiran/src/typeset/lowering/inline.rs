//! インライン要素（`resolve::ResolvedInline`）の lowering

use super::{
  LoweringContext, LoweringState,
  layout_node::{LayoutNode, TextStyle},
  math::lower_inline_math,
};
use crate::{
  config::FootnoteStyle,
  model::{AnchorId, FontKind, FootnoteId, Length, LinkTarget},
  resolve::ResolvedInline,
};

/// インライン要素をレイアウトノードに変換する
pub(super) fn lower_inline(
  ctx: &LoweringContext,
  inline: &ResolvedInline,
  parent_style: TextStyle,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  match inline {
    ResolvedInline::Text(text) => {
      return vec![LayoutNode::Text(text.clone(), parent_style)];
    },
    ResolvedInline::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
        color: parent_style.color,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, styled, state));
      }
      return result;
    },
    ResolvedInline::Colored { color, children } => {
      let colored = TextStyle {
        font_size: parent_style.font_size,
        font_kind: parent_style.font_kind,
        color: Some(*color),
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, colored, state));
      }
      return result;
    },
    ResolvedInline::InlineMath(math_nodes) => {
      return lower_inline_math(math_nodes, parent_style.font_size, &ctx.style.math.script);
    },
    ResolvedInline::Symbol(ch) => {
      return vec![LayoutNode::Text(ch.to_string(), parent_style)];
    },
    ResolvedInline::LineBreak => {
      return vec![LayoutNode::LineBreak];
    },
    ResolvedInline::NoIndent => {
      // 通常は段落変換時に除去されるが、単独変換でも描画しない。
      return Vec::new();
    },
    ResolvedInline::Index { key, .. } => {
      return vec![LayoutNode::IndexMark {
        word: key.word.clone(),
        reading: key.reading.clone(),
      }];
    },
    ResolvedInline::Ref { target, .. } => {
      // 参照先の存在と番号は `resolve` が確定させているので、ここで表示文字列まで作る。
      let style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      return vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Label(target.clone())),
        children: vec![LayoutNode::Text(
          state.ref_display(ctx.style, target),
          style,
        )],
      }];
    },
    ResolvedInline::Link { url, children } => {
      let style = with_link_color(parent_style, ctx.style.hyperref.url_color);
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, style, state));
      }
      return vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: inner,
      }];
    },
    ResolvedInline::InternalLink { target, children } => {
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, parent_style, state));
      }
      return vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Citation(target.clone())),
        children: inner,
      }];
    },
    ResolvedInline::Cite { label, .. } => {
      // `label`（CSL 整形済みインライン列）は `resolve` の時点で必ず埋まっている。
      let style = with_link_color(parent_style, ctx.style.hyperref.cite_color);
      let mut result = Vec::new();
      for child in label {
        result.extend(lower_inline(ctx, child, style, state));
      }
      return result;
    },
    ResolvedInline::Footnote { body, .. } => {
      let index = state.next_footnote_index();
      let number = footnote_number(ctx, index);
      let footnote_style = &ctx.style.footnote;
      let marker_text = super::placeholder::expand(&footnote_style.marker_format, |name| match name {
        "number" => return footnote_style.number_style.render(number),
        _ => return format!("{{{name}}}"),
      });

      // 本文中のマーカーから脚注本体へリンクする。
      let link_style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      let inline_marker = LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Footnote(FootnoteId::new(index))),
        children: vec![footnote_marker_node(
          &marker_text,
          parent_style.font_size,
          link_style,
          footnote_style,
        )],
      };

      let body_style = TextStyle {
        font_size: footnote_style.font_size,
        font_kind: parent_style.font_kind,
        color: parent_style.color,
      };
      let body_marker = footnote_marker_node(&marker_text, footnote_style.font_size, body_style, footnote_style);
      let mut lowered_body = vec![body_marker];
      for child in body {
        lowered_body.extend(lower_inline(ctx, child, body_style, state));
      }

      return vec![
        inline_marker,
        LayoutNode::Footnote {
          number,
          index,
          body: lowered_body,
        },
      ];
    },
  }
}

/// 出現 index の脚注に振る表示番号を返す
fn footnote_number(ctx: &LoweringContext, index: u32) -> u32 {
  let continuous = index + 1;
  return ctx
    .footnote_numbers
    .and_then(|numbers| return numbers.get(index as usize).copied())
    .unwrap_or(continuous);
}

/// 脚注マーカー（上付き番号）1 個を `LayoutNode::Raise` で組み立てる
fn footnote_marker_node(
  marker_text: &str,
  base_font_size: Length,
  base_style: TextStyle,
  footnote_style: &FootnoteStyle,
) -> LayoutNode {
  let marker_style = TextStyle {
    font_size: base_font_size * footnote_style.marker_size_factor,
    font_kind: FontKind::Serif,
    color: base_style.color,
  };
  return LayoutNode::Raise {
    offset: base_font_size * footnote_style.marker_raise_factor,
    children: vec![LayoutNode::Text(marker_text.to_string(), marker_style)],
  };
}

/// リンク表示テキストにハイパーリンク色を適用したスタイルを返す。
fn with_link_color(parent_style: TextStyle, link_color: Option<crate::model::Color>) -> TextStyle {
  return TextStyle {
    color: parent_style.color.or(link_color),
    ..parent_style
  };
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{super::test_support, *};
  use crate::{
    config::CounterName,
    model::{LabelId, Length, Span},
    resolve::{self, CounterKind, CounterValue, ResolvedDocument},
  };

  /// `sec:intro`（既定スタイルで section = 1.1）だけを登録した解決済みドキュメントを作る
  fn document_with_section() -> ResolvedDocument {
    return test_support::document(&[(
      "sec:intro",
      CounterValue {
        kind: CounterKind::Counter(CounterName::Section),
        parts: vec![0, 1, 1],
      },
    )]);
  }

  /// `sec:intro` を指す解決済み `\ref` を作る
  fn ref_inline() -> ResolvedInline {
    return ResolvedInline::Ref {
      target: LabelId::new("sec:intro"),
      span: Span::DUMMY,
    };
  }

  /// 脚注 1 個ぶんの解決済みインラインを作る
  fn footnote_inline(text: &str) -> ResolvedInline {
    return ResolvedInline::Footnote {
      body: vec![ResolvedInline::Text(text.to_string())],
      span: Span::DUMMY,
    };
  }

  #[test]
  fn lower_inline_styled_overrides_parent_kind() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Styled {
      kind: crate::model::FontKind::SerifItalic,
      children: vec![ResolvedInline::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: Length::pt(10.0),
      font_kind: crate::model::FontKind::SerifBold,
      color: None,
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, crate::model::FontKind::SerifItalic);
    assert_eq!(text_style.font_size, Length::pt(10.0));
  }

  #[test]
  fn lower_inline_colored_overrides_color_keeps_font() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Colored {
      color: crate::model::Color::new(0xff, 0x00, 0x00),
      children: vec![ResolvedInline::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: Length::pt(10.0),
      font_kind: crate::model::FontKind::SansSerif,
      color: None,
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, crate::model::FontKind::SansSerif);
    assert_eq!(text_style.color, Some(crate::model::Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn lower_bold_inside_color_keeps_color() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Colored {
      color: crate::model::Color::new(0x00, 0x80, 0x00),
      children: vec![ResolvedInline::Styled {
        kind: crate::model::FontKind::SerifBold,
        children: vec![ResolvedInline::Text("x".to_string())],
      }],
    };
    let parent = TextStyle::new(Length::pt(10.0));
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Text(_, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text_style.font_kind, crate::model::FontKind::SerifBold);
    assert_eq!(text_style.color, Some(crate::model::Color::new(0x00, 0x80, 0x00)));
  }

  #[test]
  fn lower_ref_resolves_to_internal_link_with_display_number() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let document = document_with_section();
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &ref_inline(), parent, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Label(LabelId::new("sec:intro"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "Section 1.1"), "{children:?}");
  }

  #[test]
  fn lower_external_link_maps_to_external_target() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Link {
      url: "https://example.com".to_string(),
      children: vec![ResolvedInline::Text("ここ".to_string())],
    };
    let parent = TextStyle::new(Length::pt(10.0));
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::External("https://example.com".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "ここ"));
  }

  /// 解決済み `\ref` の表示テキストに付いたスタイルを取り出すテストヘルパ
  fn ref_text_style(nodes: &[LayoutNode]) -> TextStyle {
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    return *text_style;
  }

  #[test]
  fn lower_ref_applies_link_color() {
    // Arrange
    let blue = crate::model::Color::new(0x00, 0x00, 0xff);
    let mut style = crate::config::Style::default();
    style.hyperref.link_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let document = document_with_section();

    // Act
    let nodes = lower_inline(&ctx, &ref_inline(), TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    assert_eq!(ref_text_style(&nodes).color, Some(blue));
  }

  #[test]
  fn lower_external_link_applies_url_color() {
    // Arrange
    let blue = crate::model::Color::new(0x00, 0x00, 0xff);
    let mut style = crate::config::Style::default();
    style.hyperref.url_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Link {
      url: "https://example.com".to_string(),
      children: vec![ResolvedInline::Text("ここ".to_string())],
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_ref_inherits_black_when_link_color_none() {
    // Arrange
    let mut style = crate::config::Style::default();
    style.hyperref.link_color = None;
    let ctx = LoweringContext::new(&style);
    let document = document_with_section();

    // Act
    let nodes = lower_inline(&ctx, &ref_inline(), TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    assert_eq!(ref_text_style(&nodes).color, None);
  }

  #[test]
  fn lower_explicit_color_overrides_link_color() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let red = crate::model::Color::new(0xff, 0x00, 0x00);
    let inline = ResolvedInline::Colored {
      color: red,
      children: vec![ref_inline()],
    };
    let document = document_with_section();

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    assert_eq!(ref_text_style(&nodes).color, Some(red));
  }

  #[test]
  fn lower_internal_link_maps_to_internal_target() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::InternalLink {
      target: crate::model::CitationId::new("foo"),
      children: vec![ResolvedInline::Text("1".to_string())],
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(crate::model::CitationId::new("foo"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "1"));
  }

  #[test]
  fn lower_cite_label_applies_cite_color_and_links() {
    // Arrange
    let blue = crate::model::Color::new(0x00, 0x00, 0xff);
    let mut style = crate::config::Style::default();
    style.hyperref.cite_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Cite {
      targets: vec![crate::model::CitationId::new("foo")],
      label: vec![ResolvedInline::InternalLink {
        target: crate::model::CitationId::new("foo"),
        children: vec![ResolvedInline::Text("1".to_string())],
      }],
      span: Span::DUMMY,
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(crate::model::CitationId::new("foo"))));
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_footnote_assigns_sequential_number_and_lowers_body() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(
      &ctx,
      &footnote_inline("note"),
      TextStyle::new(Length::pt(10.0)),
      &mut LoweringState::new(&document),
    );

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert!(matches!(&children[0], LayoutNode::Raise { .. }));
    let LayoutNode::Footnote {
      number,
      index,
      body,
    } = &nodes[1]
    else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    assert_eq!(*number, 1);
    assert_eq!(*index, 0);
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Footnote(crate::model::FootnoteId::new(*index))));
    assert!(matches!(&body[0], LayoutNode::Raise { .. }));
    assert!(matches!(&body[1], LayoutNode::Text(t, _) if t == "note"));
  }

  /// 脚注マーカー（`LayoutNode::Raise` + `Text`。本文中マーカーは `Link` で包まれているので、
  /// あれば先に剥がしてから読む）の表示テキストを取り出すテストヘルパ
  fn marker_text(node: &LayoutNode) -> &str {
    let node = match node {
      LayoutNode::Link { children, .. } => &children[0],
      other => other,
    };
    let LayoutNode::Raise { children, .. } = node else {
      panic!("Raise が期待されます: {node:?}");
    };
    let LayoutNode::Text(text, _) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    return text;
  }

  #[test]
  fn lower_footnote_applies_number_override_to_both_markers() {
    // Arrange
    let style = crate::config::Style::default();
    let numbers = [1, 1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    state.next_footnote_index(); // 1 個目は本文側で採番済みという想定

    // Act
    let nodes = lower_inline(&ctx, &footnote_inline("note"), TextStyle::new(Length::pt(10.0)), &mut state);

    // Assert
    let LayoutNode::Footnote {
      number,
      index,
      body,
    } = &nodes[1]
    else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    assert_eq!(*number, 1);
    assert_eq!(*index, 1);
    assert_eq!(marker_text(&nodes[0]), "1");
    assert_eq!(marker_text(&body[0]), "1");
  }

  #[test]
  fn lower_footnote_falls_back_to_continuous_number_outside_override_map() {
    // Arrange
    let style = crate::config::Style::default();
    let numbers = [1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    state.next_footnote_index(); // index 0 は消費済み。この脚注は index 1 = マップの範囲外

    // Act
    let nodes = lower_inline(&ctx, &footnote_inline("note"), TextStyle::new(Length::pt(10.0)), &mut state);

    // Assert
    assert!(
      matches!(
        &nodes[1],
        LayoutNode::Footnote {
          number: 2,
          index: 1,
          ..
        }
      ),
      "{nodes:?}"
    );
  }

  #[test]
  fn lower_footnote_body_preserves_nested_styling() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Footnote {
      body: vec![ResolvedInline::Styled {
        kind: crate::model::FontKind::SerifBold,
        children: vec![ResolvedInline::Text("x".to_string())],
      }],
      span: Span::DUMMY,
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Footnote { body, .. } = &nodes[1] else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(text, text_style) = &body[1] else {
      panic!("Text が期待されます: {body:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, crate::model::FontKind::SerifBold);
  }

  #[test]
  fn lower_footnote_increments_state_across_calls() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);

    // Act
    let first = lower_inline(&ctx, &footnote_inline("a"), TextStyle::new(Length::pt(10.0)), &mut state);
    let second = lower_inline(&ctx, &footnote_inline("b"), TextStyle::new(Length::pt(10.0)), &mut state);

    // Assert
    assert!(matches!(&first[1], LayoutNode::Footnote { number: 1, .. }));
    assert!(matches!(&second[1], LayoutNode::Footnote { number: 2, .. }));
  }

  #[test]
  fn lower_footnote_reflects_non_default_footnote_style() {
    // Arrange
    let mut style = crate::config::Style::default();
    style.footnote.font_size = Length::pt(20.0);
    style.footnote.marker_size_factor = 0.5;
    style.footnote.marker_format = "[{number}]".to_string();
    let ctx = LoweringContext::new(&style);
    let document = test_support::document(&[]);
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &footnote_inline("note"), parent_style, &mut LoweringState::new(&document));

    // Assert
    let LayoutNode::Link {
      children: link_children,
      ..
    } = &nodes[0]
    else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Raise { children, .. } = &link_children[0] else {
      panic!("Raise が期待されます: {link_children:?}");
    };
    let LayoutNode::Text(marker_text, marker_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(marker_text, "[1]");
    assert!((marker_style.font_size.to_pt() - 5.0).abs() < 1e-3, "{}", marker_style.font_size.to_pt());

    // Assert
    let LayoutNode::Footnote { body, .. } = &nodes[1] else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    let LayoutNode::Raise { children, .. } = &body[0] else {
      panic!("Raise が期待されます: {body:?}");
    };
    let LayoutNode::Text(_, body_marker_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert!((body_marker_style.font_size.to_pt() - 10.0).abs() < 1e-3, "{}", body_marker_style.font_size.to_pt());
    let LayoutNode::Text(_, body_text_style) = &body[1] else {
      panic!("Text が期待されます: {body:?}");
    };
    assert!((body_text_style.font_size.to_pt() - 20.0).abs() < 1e-3, "{}", body_text_style.font_size.to_pt());
  }

  #[test]
  fn lower_footnote_applies_number_style_to_both_markers() {
    // Arrange
    let mut style = crate::config::Style::default();
    style.footnote.number_style = crate::config::NumberStyle::RomanUpper;
    let ctx = LoweringContext::new(&style);
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let first = lower_inline(&ctx, &footnote_inline("a"), parent_style, &mut state);
    let second = lower_inline(&ctx, &footnote_inline("b"), parent_style, &mut state);

    // Assert
    let LayoutNode::Footnote {
      body: first_body, ..
    } = &first[1]
    else {
      panic!("Footnote が期待されます: {first:?}");
    };
    assert_eq!(marker_text(&first[0]), "I");
    assert_eq!(marker_text(&first_body[0]), "I");

    let LayoutNode::Footnote {
      body: second_body, ..
    } = &second[1]
    else {
      panic!("Footnote が期待されます: {second:?}");
    };
    assert_eq!(marker_text(&second[0]), "II");
    assert_eq!(marker_text(&second_body[0]), "II");
  }

  #[test]
  fn lower_inline_index_produces_index_mark_layout_node() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Index {
      key: resolve::IndexKey {
        word: "語".to_string(),
        reading: None,
      },
      span: Span::DUMMY,
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(
      &nodes[0],
      LayoutNode::IndexMark { word, reading } if word == "語" && reading.is_none()
    ));
  }

  #[test]
  fn lower_inline_index_with_reading_preserves_reading() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Index {
      key: resolve::IndexKey {
        word: "語".to_string(),
        reading: Some("よみ".to_string()),
      },
      span: Span::DUMMY,
    };
    let document = test_support::document(&[]);

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut LoweringState::new(&document));

    // Assert
    assert!(matches!(
      &nodes[0],
      LayoutNode::IndexMark { reading, .. } if reading.as_deref() == Some("よみ")
    ));
  }

  #[test]
  fn lower_inline_index_does_not_consume_footnote_counter() {
    // Arrange
    let style = crate::config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = ResolvedInline::Index {
      key: resolve::IndexKey {
        word: "語".to_string(),
        reading: None,
      },
      span: Span::DUMMY,
    };
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    lower_inline(&ctx, &inline, parent_style, &mut state);

    // Assert
    let nodes = lower_inline(&ctx, &footnote_inline("a"), parent_style, &mut state);
    assert_eq!(marker_text(&nodes[0]), "1");
  }
}
