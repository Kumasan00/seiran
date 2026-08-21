//! インライン要素（`document::HirInline`）の lowering

use super::{
  LoweringContext, LoweringState, generated,
  layout_node::{LayoutNode, TextStyle},
  math::lower_inline_math,
};
use crate::{
  document::{FontKind, HirInline, HirInlineKind},
  length::Length,
  style::FootnoteStyle,
  typeset::boxes::{AnchorId, FootnoteId, LinkTarget},
};

/// インライン列をまとめてレイアウトノードに変換する
pub(super) fn lower_inlines(
  ctx: &LoweringContext,
  inlines: &[HirInline],
  parent_style: TextStyle,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for inline in inlines {
    result.extend(lower_inline(ctx, inline, parent_style, state));
  }
  return result;
}

/// インライン要素をレイアウトノードに変換する
pub(super) fn lower_inline(
  ctx: &LoweringContext,
  inline: &HirInline,
  parent_style: TextStyle,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  match &inline.kind {
    HirInlineKind::Text(text) => {
      return vec![LayoutNode::Text(text.clone(), parent_style)];
    },
    HirInlineKind::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
        color: parent_style.color,
      };
      return lower_inlines(ctx, children, styled, state);
    },
    HirInlineKind::Colored { color, children } => {
      let colored = TextStyle {
        font_size: parent_style.font_size,
        font_kind: parent_style.font_kind,
        color: Some(*color),
      };
      return lower_inlines(ctx, children, colored, state);
    },
    HirInlineKind::InlineMath(math_nodes) => {
      return lower_inline_math(math_nodes, parent_style.font_size, &ctx.style.math.script);
    },
    HirInlineKind::Symbol(ch) => {
      return vec![LayoutNode::Text(ch.to_string(), parent_style)];
    },
    HirInlineKind::LineBreak => {
      return vec![LayoutNode::LineBreak];
    },
    HirInlineKind::NoIndent => {
      // 通常は段落変換時に除去されるが、単独変換でも描画しない。
      return Vec::new();
    },
    HirInlineKind::Index { word, reading } => {
      return vec![LayoutNode::IndexMark {
        word: word.clone(),
        reading: reading.clone(),
      }];
    },
    HirInlineKind::Ref { .. } => {
      // 参照先の存在と番号は `semantics::analyze` が確定させているので、ここで表示文字列まで作る。
      let target = state.reference_target(inline.id);
      let style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      return vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Label(target.clone())),
        children: vec![LayoutNode::Text(
          state.ref_display(ctx.style, target),
          style,
        )],
      }];
    },
    HirInlineKind::Link { url, children } => {
      let style = with_link_color(parent_style, ctx.style.hyperref.url_color);
      let inner = lower_inlines(ctx, children, style, state);
      return vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: inner,
      }];
    },
    HirInlineKind::Cite { .. } => {
      // 表示（CSL 整形済みインライン列）は文書木ではなく生成物の side table から引く。
      // 生成物は `NodeId` を持たないので、著者が書いた本文とは別経路で lower する。
      let style = with_link_color(parent_style, ctx.style.hyperref.cite_color);
      return generated::lower_generated_inlines(ctx, state.citation_display(inline.id), style);
    },
    HirInlineKind::Footnote { body } => {
      let index = state.next_footnote_index();
      let number = footnote_number(ctx, index);
      let footnote_style = &ctx.style.footnote;
      let marker_text = footnote_style.marker_format.expand(&footnote_style.number_style.render(number));

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
pub(super) fn with_link_color(parent_style: TextStyle, link_color: Option<crate::color::Color>) -> TextStyle {
  return TextStyle {
    color: parent_style.color.or(link_color),
    ..parent_style
  };
}

#[cfg(test)]
mod tests {
  use super::{
    super::{
      lower_sources_with_headings,
      test_support::{analyzed, lower},
    },
    *,
  };
  use crate::{
    color::Color,
    semantics::{CitationId, GeneratedInline, LabelId},
    style::{NumberTemplate, Style as ReadStyle},
  };

  /// `.sei` ソースを lower してレイアウトノード列を返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  /// 与えた文脈で `.sei` ソースを lower するテストヘルパ（脚注番号の上書きを使うテスト用）
  fn lower_source_with(ctx: &LoweringContext, source: &str) -> Vec<LayoutNode> {
    let (layout, _headings) = lower_sources_with_headings(ctx, &analyzed(source));
    return layout;
  }

  /// レイアウトノード列から最初の `Text`（文字列とスタイル）を取り出す
  fn first_text(nodes: &[LayoutNode]) -> (&str, TextStyle) {
    let LayoutNode::Text(text, style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    return (text, *style);
  }

  /// レイアウトノード列から最初の `Link`（ターゲットと子）を取り出す
  fn first_link(nodes: &[LayoutNode]) -> (&LinkTarget, &[LayoutNode]) {
    let link = nodes.iter().find_map(|n| match n {
      LayoutNode::Link { target, children } => return Some((target, children.as_slice())),
      _ => return None,
    });
    return link.expect("Link が期待されます");
  }

  /// レイアウトノード列から最初の `Footnote` を取り出す
  fn first_footnote(nodes: &[LayoutNode]) -> (u32, u32, &[LayoutNode]) {
    let footnote = nodes.iter().find_map(|n| match n {
      LayoutNode::Footnote {
        number,
        index,
        body,
      } => return Some((*number, *index, body.as_slice())),
      _ => return None,
    });
    return footnote.expect("Footnote が期待されます");
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
  fn lower_inline_styled_overrides_parent_kind() {
    // Arrange
    let mut style = ReadStyle::default();
    style.text.font_kind = FontKind::SerifBold;

    // Act
    let nodes = lower_source(&style, "\\italic{x}\n");

    // Assert
    let (text, text_style) = first_text(&nodes);
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, FontKind::SerifItalic);
    assert_eq!(text_style.font_size, style.text.font_size);
  }

  #[test]
  fn lower_inline_colored_overrides_color_keeps_font() {
    // Arrange
    let mut style = ReadStyle::default();
    style.text.font_kind = FontKind::SansSerif;

    // Act
    let nodes = lower_source(&style, "\\color[color=#ff0000]{x}\n");

    // Assert
    let (text, text_style) = first_text(&nodes);
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, FontKind::SansSerif);
    assert_eq!(text_style.color, Some(Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn lower_bold_inside_color_keeps_color() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\color[color=#008000]{\\bold{x}}\n");

    // Assert
    let (_, text_style) = first_text(&nodes);
    assert_eq!(text_style.font_kind, FontKind::SerifBold);
    assert_eq!(text_style.color, Some(Color::new(0x00, 0x80, 0x00)));
  }

  #[test]
  fn lower_ref_resolves_to_internal_link_with_display_number() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\chapter{C}\n\n\\section[label=sec:intro]{S}\n\n\\ref{sec:intro}\n");

    // Assert
    let (target, children) = first_link(&nodes);
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Label(LabelId::new("sec:intro"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "Section 1.1"), "{children:?}");
  }

  #[test]
  fn lower_external_link_maps_to_external_target() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\href[url=https:\\/\\/example.com]{ここ}\n");

    // Assert
    let (target, children) = first_link(&nodes);
    assert_eq!(*target, LinkTarget::External("https://example.com".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "ここ"));
  }

  /// 解決済み `\ref` の表示テキストに付いたスタイルを取り出すテストヘルパ
  fn ref_text_style(nodes: &[LayoutNode]) -> TextStyle {
    let (_, children) = first_link(nodes);
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    return *text_style;
  }

  /// ラベル付き見出しとそれを指す `\ref` だけを含むソース
  const REF_SOURCE: &str = "\\chapter{C}\n\n\\section[label=sec:intro]{S}\n\n\\ref{sec:intro}\n";

  #[test]
  fn lower_ref_applies_link_color() {
    // Arrange
    let blue = Color::new(0x00, 0x00, 0xff);
    let mut style = ReadStyle::default();
    style.hyperref.link_color = Some(blue);

    // Act
    let nodes = lower_source(&style, REF_SOURCE);

    // Assert
    assert_eq!(ref_text_style(&nodes).color, Some(blue));
  }

  #[test]
  fn lower_external_link_applies_url_color() {
    // Arrange
    let blue = Color::new(0x00, 0x00, 0xff);
    let mut style = ReadStyle::default();
    style.hyperref.url_color = Some(blue);

    // Act
    let nodes = lower_source(&style, "\\href[url=https:\\/\\/example.com]{ここ}\n");

    // Assert
    let (_, children) = first_link(&nodes);
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_ref_inherits_black_when_link_color_none() {
    // Arrange
    let mut style = ReadStyle::default();
    style.hyperref.link_color = None;

    // Act
    let nodes = lower_source(&style, REF_SOURCE);

    // Assert
    assert_eq!(ref_text_style(&nodes).color, None);
  }

  #[test]
  fn lower_explicit_color_overrides_link_color() {
    // Arrange
    let red = Color::new(0xff, 0x00, 0x00);
    let mut style = ReadStyle::default();
    style.hyperref.link_color = Some(Color::new(0x00, 0x00, 0xff));

    // Act
    let nodes = lower_source(
      &style,
      "\\chapter{C}\n\n\\section[label=sec:intro]{S}\n\n\\color[color=#ff0000]{\\ref{sec:intro}}\n",
    );

    // Assert
    assert_eq!(ref_text_style(&nodes).color, Some(red));
  }

  #[test]
  fn lower_cite_label_applies_cite_color_and_links() {
    // Arrange — 引用の表示（CSL 整形の生成物）は side table 側から与える
    let blue = Color::new(0x00, 0x00, 0xff);
    let mut style = ReadStyle::default();
    style.hyperref.cite_color = Some(blue);
    let analyzed = analyzed("\\cite{kwan2014}\n");
    let site = analyzed.citation_sites().next().expect("引用箇所が 1 件あるはず");
    let document = analyzed.with_citations_for_test(
      vec![(
        site,
        vec![GeneratedInline::InternalLink {
          target: CitationId::new("kwan2014"),
          children: vec![GeneratedInline::Text("1".to_string())],
        }],
      )],
      Vec::new(),
    );

    // Act
    let nodes = lower(&style, &document);

    // Assert
    let (target, children) = first_link(&nodes);
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(CitationId::new("kwan2014"))));
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_footnote_assigns_sequential_number_and_lowers_body() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\footnote{note}\n");

    // Assert
    let (target, children) = first_link(&nodes);
    assert!(matches!(&children[0], LayoutNode::Raise { .. }));
    let (number, index, body) = first_footnote(&nodes);
    assert_eq!(number, 1);
    assert_eq!(index, 0);
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Footnote(FootnoteId::new(index))));
    assert!(matches!(&body[0], LayoutNode::Raise { .. }));
    assert!(matches!(&body[1], LayoutNode::Text(t, _) if t == "note"));
  }

  #[test]
  fn lower_footnote_applies_number_override_to_both_markers() {
    // Arrange — ページ単位採番で 2 個目の脚注も 1 番になる場合を模す
    let style = ReadStyle::default();
    let numbers = [1, 1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);

    // Act
    let nodes = lower_source_with(&ctx, "a\\footnote{first}\n\nb\\footnote{note}\n");

    // Assert — 2 個目の脚注（index 1）の表示番号は上書きされて 1
    let second = &nodes[nodes.iter().rposition(|n| matches!(n, LayoutNode::Footnote { .. })).expect("脚注あり")];
    let LayoutNode::Footnote {
      number,
      index,
      body,
    } = second
    else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    assert_eq!(*number, 1);
    assert_eq!(*index, 1);
    assert_eq!(marker_text(&body[0]), "1", "脚注エリア側のマーカー");
    // Assert — 本文側マーカーも同じ上書き番号になる（1 個目・2 個目とも 1 番）
    assert_eq!(text_side_markers(&nodes), vec![(0, "1"), (1, "1")], "本文側のマーカー: {nodes:?}");
  }

  #[test]
  fn lower_footnote_falls_back_to_continuous_number_outside_override_map() {
    // Arrange — 上書きマップは index 0 しか持たない
    let style = ReadStyle::default();
    let numbers = [1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);

    // Act
    let nodes = lower_source_with(&ctx, "a\\footnote{first}\n\nb\\footnote{note}\n");

    // Assert
    let last = &nodes[nodes.iter().rposition(|n| matches!(n, LayoutNode::Footnote { .. })).expect("脚注あり")];
    assert!(
      matches!(
        last,
        LayoutNode::Footnote {
          number: 2,
          index: 1,
          ..
        }
      ),
      "{nodes:?}"
    );
  }

  /// 本文中の脚注マーカー（`AnchorId::Footnote` を指す `Link`）を、脚注 index と表示テキストの
  /// 組で文書順に集めるテストヘルパ
  ///
  /// 本文側マーカーの幅はページ単位採番の不動点計算に効くので、脚注エリア側だけでなく
  /// こちらも検証する。
  fn text_side_markers(nodes: &[LayoutNode]) -> Vec<(u32, &str)> {
    return nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Link {
          target: LinkTarget::Internal(AnchorId::Footnote(id)),
          ..
        } => return Some((id.index(), marker_text(n))),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn lower_footnote_body_preserves_nested_styling() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\footnote{\\bold{x}}\n");

    // Assert
    let (_, _, body) = first_footnote(&nodes);
    let LayoutNode::Text(text, text_style) = &body[1] else {
      panic!("Text が期待されます: {body:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, FontKind::SerifBold);
  }

  #[test]
  fn lower_footnote_increments_state_across_calls() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "a\\footnote{a}\n\nb\\footnote{b}\n");

    // Assert
    let numbers: Vec<u32> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Footnote { number, .. } => return Some(*number),
        _ => return None,
      })
      .collect();
    assert_eq!(numbers, vec![1, 2], "{nodes:?}");
  }

  #[test]
  fn lower_footnote_reflects_non_default_footnote_style() {
    // Arrange
    let mut style = ReadStyle::default();
    style.footnote.font_size = Length::pt(20.0);
    style.footnote.marker_size_factor = 0.5;
    style.footnote.marker_format = NumberTemplate::parse("[{number}]");

    // Act
    let nodes = lower_source(&style, "\\footnote{note}\n");

    // Assert
    let (_, link_children) = first_link(&nodes);
    let LayoutNode::Raise { children, .. } = &link_children[0] else {
      panic!("Raise が期待されます: {link_children:?}");
    };
    let LayoutNode::Text(marker, marker_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(marker, "[1]");
    let body_font_size = style.text.font_size.to_pt();
    assert!(
      (marker_style.font_size.to_pt() - body_font_size * 0.5).abs() < 1e-3,
      "{}",
      marker_style.font_size.to_pt()
    );

    // Assert — 脚注本体側のマーカーは脚注フォントサイズ基準
    let (_, _, body) = first_footnote(&nodes);
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
    let mut style = ReadStyle::default();
    style.footnote.number_style = crate::style::NumberStyle::RomanUpper;

    // Act
    let nodes = lower_source(&style, "a\\footnote{a}\n\nb\\footnote{b}\n");

    // Assert — 脚注エリア側
    let area_markers: Vec<&str> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Footnote { body, .. } => return Some(marker_text(&body[0])),
        _ => return None,
      })
      .collect();
    assert_eq!(area_markers, vec!["I", "II"], "脚注エリア側のマーカー: {nodes:?}");

    // Assert — 本文側
    assert_eq!(text_side_markers(&nodes), vec![(0, "I"), (1, "II")], "本文側のマーカー: {nodes:?}");
  }

  #[test]
  fn lower_inline_index_produces_index_mark_layout_node() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\index{語}\n");

    // Assert
    assert!(
      matches!(&nodes[0], LayoutNode::IndexMark { word, reading } if word == "語" && reading.is_none()),
      "{nodes:?}"
    );
  }

  #[test]
  fn lower_inline_index_with_reading_preserves_reading() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower_source(&style, "\\index[reading=よみ]{語}\n");

    // Assert
    assert!(
      matches!(&nodes[0], LayoutNode::IndexMark { reading, .. } if reading.as_deref() == Some("よみ")),
      "{nodes:?}"
    );
  }

  #[test]
  fn lower_inline_index_does_not_consume_footnote_counter() {
    // Arrange
    let style = ReadStyle::default();

    // Act — 索引マーカーの後の脚注が 1 番のままであることを見る
    let nodes = lower_source(&style, "\\index{語}\\footnote{a}\n");

    // Assert
    let (number, _, _) = first_footnote(&nodes);
    assert_eq!(number, 1, "{nodes:?}");
  }
}
