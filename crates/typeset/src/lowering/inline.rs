//! インライン要素（`InlineNode`）の lowering

use config::FootnoteStyle;
use model::{AnchorId, FontKind, FootnoteId, InlineNode, Length, LinkTarget};

use super::{
  LoweringContext, LoweringError,
  counter::CounterRegistry,
  layout_node::{LayoutNode, TextStyle},
  math::lower_inline_math,
};

/// インライン要素をレイアウトノードに変換する
pub(super) fn lower_inline(
  ctx: &LoweringContext,
  inline: &InlineNode,
  parent_style: TextStyle,
  registry: &mut CounterRegistry,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match inline {
    InlineNode::Text(text) => {
      return Ok(vec![LayoutNode::Text(text.clone(), parent_style)]);
    },
    InlineNode::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
        color: parent_style.color,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, styled, registry)?);
      }
      return Ok(result);
    },
    InlineNode::Colored { color, children } => {
      let colored = TextStyle {
        font_size: parent_style.font_size,
        font_kind: parent_style.font_kind,
        color: Some(*color),
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, colored, registry)?);
      }
      return Ok(result);
    },
    InlineNode::InlineMath(math_nodes) => {
      return Ok(lower_inline_math(math_nodes, parent_style.font_size, &ctx.style.math.script));
    },
    InlineNode::Symbol(ch) => {
      return Ok(vec![LayoutNode::Text(ch.to_string(), parent_style)]);
    },
    InlineNode::LineBreak => {
      return Ok(vec![LayoutNode::LineBreak]);
    },
    InlineNode::NoIndent => {
      // 通常は段落変換時に除去されるが、単独変換でも描画しない。
      return Ok(Vec::new());
    },
    InlineNode::Index { word, reading, .. } => {
      return Ok(vec![LayoutNode::IndexMark {
        word: word.clone(),
        reading: reading.clone(),
      }]);
    },
    InlineNode::Ref { label, span } => {
      // 前方参照を許すため、表示スタイルだけ確定して pass 2 へ送る。
      let style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      return Ok(vec![LayoutNode::Ref {
        label: label.clone(),
        span: super::span_to_source_span(*span),
        style,
        as_link: true,
        source: ctx.source,
      }]);
    },
    InlineNode::Link { url, children } => {
      let style = with_link_color(parent_style, ctx.style.hyperref.url_color);
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, style, registry)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: inner,
      }]);
    },
    InlineNode::InternalLink { target, children } => {
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, parent_style, registry)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Citation(target.clone())),
        children: inner,
      }]);
    },
    InlineNode::Cite { keys, label, span } => {
      // 引用整形済みであることを lowering の境界で検証する。
      let Some(inlines) = label else {
        return Err(LoweringError::UnresolvedCitation {
          keys: keys.join(", "),
          span: super::span_to_source_span(*span),
          origin: ctx.source,
        });
      };
      let style = with_link_color(parent_style, ctx.style.hyperref.cite_color);
      let mut result = Vec::new();
      for child in inlines {
        result.extend(lower_inline(ctx, child, style, registry)?);
      }
      return Ok(result);
    },
    InlineNode::Footnote { body, .. } => {
      let index = registry.next_footnote_index();
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
        lowered_body.extend(lower_inline(ctx, child, body_style, registry)?);
      }

      return Ok(vec![
        inline_marker,
        LayoutNode::Footnote {
          number,
          index,
          body: lowered_body,
        },
      ]);
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
fn with_link_color(parent_style: TextStyle, link_color: Option<model::Color>) -> TextStyle {
  return TextStyle {
    color: parent_style.color.or(link_color),
    ..parent_style
  };
}

#[cfg(test)]
mod tests {
  use model::Length;

  use super::*;

  #[test]
  fn lower_inline_styled_overrides_parent_kind() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Styled {
      kind: model::FontKind::SerifItalic,
      children: vec![InlineNode::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: Length::pt(10.0),
      font_kind: model::FontKind::SerifBold,
      color: None,
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran())
      .expect("Text のみなので失敗しないはず");

    // Assert
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, model::FontKind::SerifItalic);
    assert_eq!(text_style.font_size, Length::pt(10.0));
  }

  #[test]
  fn lower_inline_colored_overrides_color_keeps_font() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Colored {
      color: model::Color::new(0xff, 0x00, 0x00),
      children: vec![InlineNode::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: Length::pt(10.0),
      font_kind: model::FontKind::SansSerif,
      color: None,
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran())
      .expect("Text のみなので失敗しないはず");

    // Assert
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, model::FontKind::SansSerif);
    assert_eq!(text_style.color, Some(model::Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn lower_bold_inside_color_keeps_color() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Colored {
      color: model::Color::new(0x00, 0x80, 0x00),
      children: vec![InlineNode::Styled {
        kind: model::FontKind::SerifBold,
        children: vec![InlineNode::Text("x".to_string())],
      }],
    };
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran())
      .expect("Text のみなので失敗しないはず");

    // Assert
    let LayoutNode::Text(_, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text_style.font_kind, model::FontKind::SerifBold);
    assert_eq!(text_style.color, Some(model::Color::new(0x00, 0x80, 0x00)));
  }

  #[test]
  fn lower_ref_emits_placeholder_with_label_and_span() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let span = model::Span::new(3, 4);
    let inline = InlineNode::Ref {
      label: "sec:intro".to_string(),
      span,
    };
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert
    let LayoutNode::Ref {
      label,
      span: got_span,
      style,
      as_link,
      ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(label, "sec:intro");
    assert_eq!(*got_span, super::super::span_to_source_span(span));
    assert_eq!(*style, parent);
    assert!(*as_link, "\\ref はリンク領域として発行されるはず");
  }

  #[test]
  fn lower_external_link_maps_to_external_target() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Link {
      url: "https://example.com".to_string(),
      children: vec![InlineNode::Text("ここ".to_string())],
    };
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::External("https://example.com".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "ここ"));
  }

  #[test]
  fn lower_ref_applies_link_color() {
    // Arrange
    let blue = model::Color::new(0x00, 0x00, 0xff);
    let mut style = config::Style::default();
    style.hyperref.link_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "sec:intro".to_string(),
      span: model::Span::DUMMY,
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Ref {
      style: ref_style, ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(ref_style.color, Some(blue));
  }

  #[test]
  fn lower_external_link_applies_url_color() {
    // Arrange
    let blue = model::Color::new(0x00, 0x00, 0xff);
    let mut style = config::Style::default();
    style.hyperref.url_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Link {
      url: "https://example.com".to_string(),
      children: vec![InlineNode::Text("ここ".to_string())],
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

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
    let mut style = config::Style::default();
    style.hyperref.link_color = None;
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "a".to_string(),
      span: model::Span::DUMMY,
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Ref {
      style: ref_style, ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(ref_style.color, None);
  }

  #[test]
  fn lower_explicit_color_overrides_link_color() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let red = model::Color::new(0xff, 0x00, 0x00);
    let inline = InlineNode::Colored {
      color: red,
      children: vec![InlineNode::Ref {
        label: "a".to_string(),
        span: model::Span::DUMMY,
      }],
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Ref {
      style: ref_style, ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(ref_style.color, Some(red));
  }

  #[test]
  fn lower_internal_link_maps_to_internal_target() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::InternalLink {
      target: model::CitationId::new("foo"),
      children: vec![InlineNode::Text("1".to_string())],
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(model::CitationId::new("foo"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "1"));
  }

  #[test]
  fn lower_cite_label_applies_cite_color_and_links() {
    // Arrange
    let blue = model::Color::new(0x00, 0x00, 0xff);
    let mut style = config::Style::default();
    style.hyperref.cite_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Cite {
      keys: vec!["foo".to_string()],
      label: Some(vec![InlineNode::InternalLink {
        target: model::CitationId::new("foo"),
        children: vec![InlineNode::Text("1".to_string())],
      }]),
      span: model::Span::DUMMY,
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("解決済み Cite は失敗しない");

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(model::CitationId::new("foo"))));
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_footnote_assigns_sequential_number_and_lowers_body() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

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
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Footnote(model::FootnoteId::new(*index))));
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
    let style = config::Style::default();
    let numbers = [1, 1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    registry.next_footnote_index(); // 1 個目は本文側で採番済みという想定

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

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
    let style = config::Style::default();
    let numbers = [1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    registry.next_footnote_index(); // index 0 は消費済み。この脚注は index 1 = マップの範囲外

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

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
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Styled {
        kind: model::FontKind::SerifBold,
        children: vec![InlineNode::Text("x".to_string())],
      }],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert
    let LayoutNode::Footnote { body, .. } = &nodes[1] else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(text, text_style) = &body[1] else {
      panic!("Text が期待されます: {body:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, model::FontKind::SerifBold);
  }

  #[test]
  fn lower_footnote_increments_registry_across_calls() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::default_for_seiran();
    let make = |text: &str| {
      return InlineNode::Footnote {
        body: vec![InlineNode::Text(text.to_string())],
        span: model::Span::DUMMY,
      };
    };

    // Act
    let first = lower_inline(&ctx, &make("a"), TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");
    let second = lower_inline(&ctx, &make("b"), TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert
    assert!(matches!(&first[1], LayoutNode::Footnote { number: 1, .. }));
    assert!(matches!(&second[1], LayoutNode::Footnote { number: 2, .. }));
  }

  #[test]
  fn lower_footnote_reflects_non_default_footnote_style() {
    // Arrange
    let mut style = config::Style::default();
    style.footnote.font_size = Length::pt(20.0);
    style.footnote.marker_size_factor = 0.5;
    style.footnote.marker_format = "[{number}]".to_string();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent_style, &mut registry).expect("失敗しない");

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
    let mut style = config::Style::default();
    style.footnote.number_style = config::NumberStyle::RomanUpper;
    let ctx = LoweringContext::new(&style);
    let make = |text: &str| {
      return InlineNode::Footnote {
        body: vec![InlineNode::Text(text.to_string())],
        span: model::Span::DUMMY,
      };
    };
    let mut registry = CounterRegistry::default_for_seiran();
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let first = lower_inline(&ctx, &make("a"), parent_style, &mut registry).expect("失敗しない");
    let second = lower_inline(&ctx, &make("b"), parent_style, &mut registry).expect("失敗しない");

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
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Index {
      word: "語".to_string(),
      reading: None,
      span: model::Span::DUMMY,
    };
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent_style, &mut CounterRegistry::default_for_seiran())
      .expect("Index の lowering は失敗しないはず");

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
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Index {
      word: "語".to_string(),
      reading: Some("よみ".to_string()),
      span: model::Span::DUMMY,
    };
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent_style, &mut CounterRegistry::default_for_seiran())
      .expect("Index の lowering は失敗しないはず");

    // Assert
    assert!(matches!(
      &nodes[0],
      LayoutNode::IndexMark { reading, .. } if reading.as_deref() == Some("よみ")
    ));
  }

  #[test]
  fn lower_inline_index_does_not_consume_footnote_counter() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Index {
      word: "語".to_string(),
      reading: None,
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    lower_inline(&ctx, &inline, parent_style, &mut registry).expect("Index の lowering は失敗しないはず");

    // Assert
    let footnote = InlineNode::Footnote {
      body: vec![InlineNode::Text("a".to_string())],
      span: model::Span::DUMMY,
    };
    let nodes = lower_inline(&ctx, &footnote, parent_style, &mut registry).expect("失敗しない");
    assert_eq!(marker_text(&nodes[0]), "1");
  }
}
