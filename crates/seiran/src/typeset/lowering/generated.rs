//! CSL 整形の生成物（書誌・引用表示）の lowering
//!
//! 生成物は HIR ではない（`NodeId` を持たない）ので、著者が書いた本文とは別経路で lower する。
//! 箱組み（見出し・段落）そのものは本文と同じ関数を通し、この module が持つのは
//! 「`InlineNode` 列 → `LayoutNode` 列」の変換と、書誌に現れる固定形の走査だけ。

use super::{
  HeadingRecord, LoweringContext,
  heading::{self, title_style},
  inline::with_link_color,
  layout_node::{LayoutNode, TextStyle},
  paragraph::{assemble_paragraph, body_text_style},
};
use crate::model::{AnchorId, AnchorMark, DocNode, HeadingKey, InlineNode, LinkTarget, inline_nodes_to_plain_text};

/// 書誌（CSL 整形の生成物）をレイアウトノードと見出し記録へ変換する
///
/// 書誌の見出しは無採番で、本文の続きとなる `HeadingKey` を `next_heading_index` から振る。
///
/// # Panics
///
/// `citation::render` が合成しない `DocNode` を含む書誌を渡した場合にパニックします。
pub(super) fn lower_bibliography(
  ctx: &LoweringContext,
  nodes: &[DocNode],
  next_heading_index: usize,
) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
  let mut layout = Vec::with_capacity(nodes.len());
  let mut headings = Vec::new();
  let mut heading_index = next_heading_index;

  for node in nodes {
    match node {
      DocNode::Heading { level, title, .. } => {
        let key = HeadingKey::new(heading_index);
        heading_index += 1;
        let style = title_style(ctx, *level);
        let title_nodes = lower_generated_inlines(ctx, title, style);
        // 書誌の見出しは無採番（`citation::render` が `numbered: false` で合成する）なので番号は空。
        layout.extend(heading::lower_heading(ctx, *level, "", &title_nodes, None, key));
        headings.push(HeadingRecord {
          index: key.index(),
          level: *level,
          number: String::new(),
          title_plain: inline_nodes_to_plain_text(title),
        });
      },
      DocNode::Paragraph(inlines) => {
        let content = lower_generated_inlines(ctx, inlines, body_text_style(ctx));
        layout.extend(assemble_paragraph(ctx, content, false));
      },
      DocNode::Anchor(target) => layout.push(LayoutNode::Anchor(AnchorMark::Citation(target.clone()))),
      other => unreachable!("書誌は citation::render が合成する固定形しか含まない: {other:?}"),
    }
  }

  return (layout, headings);
}

/// 生成物のインライン列（CSL 整形の出力）をレイアウトノードへ変換する
///
/// 生成物には `\ref` も `\cite` も索引も脚注も現れない（`citation::render` が作るのはテキスト・
/// 書体・色・リンクだけ）ので、事実を引く必要がなく `LoweringState` を取らない。
///
/// # Panics
///
/// `citation::render` が生成しない `InlineNode` を渡した場合にパニックします。
pub(super) fn lower_generated_inlines(
  ctx: &LoweringContext,
  inlines: &[InlineNode],
  parent_style: TextStyle,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for inline in inlines {
    result.extend(lower_generated_inline(ctx, inline, parent_style));
  }
  return result;
}

/// 生成物のインライン 1 個をレイアウトノードへ変換する
fn lower_generated_inline(ctx: &LoweringContext, inline: &InlineNode, parent_style: TextStyle) -> Vec<LayoutNode> {
  match inline {
    InlineNode::Text(text) => return vec![LayoutNode::Text(text.clone(), parent_style)],
    InlineNode::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
        color: parent_style.color,
      };
      return lower_generated_inlines(ctx, children, styled);
    },
    InlineNode::Colored { color, children } => {
      let colored = TextStyle {
        font_size: parent_style.font_size,
        font_kind: parent_style.font_kind,
        color: Some(*color),
      };
      return lower_generated_inlines(ctx, children, colored);
    },
    InlineNode::InternalLink { target, children } => {
      return vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Citation(target.clone())),
        children: lower_generated_inlines(ctx, children, parent_style),
      }];
    },
    InlineNode::Link { url, children } => {
      let style = with_link_color(parent_style, ctx.style.hyperref.url_color);
      return vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: lower_generated_inlines(ctx, children, style),
      }];
    },
    InlineNode::Symbol(ch) => return vec![LayoutNode::Text(ch.to_string(), parent_style)],
    InlineNode::LineBreak => return vec![LayoutNode::LineBreak],
    other => unreachable!("citation::render は生成物にこの variant を作らない: {other:?}"),
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{
    super::test_support::{analyzed, lower},
    *,
  };
  use crate::{
    config::Style as ReadStyle,
    model::{CitationId, NodeMap},
  };

  /// `citation::render` が合成するのと同じ形の書誌（見出し + アンカー + 段落）を作る
  fn bibliography() -> Vec<DocNode> {
    return vec![
      DocNode::Heading {
        level: crate::model::HeadingLevel::Section,
        numbered: false,
        title: vec![InlineNode::Text("References".to_string())],
        label: None,
        span: crate::model::Span::DUMMY,
      },
      DocNode::Anchor(CitationId::new("kwan2014")),
      DocNode::Paragraph(vec![
        InlineNode::Text("K. Kwan, ".to_string()),
        InlineNode::Styled {
          kind: crate::model::FontKind::SerifItalic,
          children: vec![InlineNode::Text("Crazy Rich Asians".to_string())],
        },
      ]),
    ];
  }

  #[test]
  fn bibliography_is_appended_after_body_with_continuing_heading_key() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed("\\section{本文}\n");
    let ctx = LoweringContext::new(&style);

    // Act
    let (layout, headings) = super::super::lower_sources_with_headings(
      &ctx,
      super::super::DocumentContent {
        analyzed: &analyzed,
        citation_displays: &NodeMap::default(),
        bibliography: &bibliography(),
      },
    );

    // Assert — 書誌の見出しは本文の見出しの続きの key を持ち、番号は空
    assert_eq!(headings.len(), 2, "{headings:?}");
    assert_eq!(headings[1].index, 1, "書誌見出しは本文の続きの index: {headings:?}");
    assert_eq!(headings[1].number, "", "書誌の見出しは無採番");
    assert_eq!(headings[1].title_plain, "References");
    let keys: Vec<usize> = layout
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Anchor(AnchorMark::Heading { key, .. }) => return Some(key.index()),
        _ => return None,
      })
      .collect();
    assert_eq!(keys, vec![0, 1], "見出しアンカーは文書順の連番: {layout:?}");
  }

  #[test]
  fn bibliography_entry_anchor_becomes_citation_anchor() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let (layout, _headings) = lower_bibliography(&LoweringContext::new(&style), &bibliography(), 0);

    // Assert
    assert!(
      layout
        .iter()
        .any(|n| matches!(n, LayoutNode::Anchor(AnchorMark::Citation(k)) if k.as_str() == "kwan2014")),
      "{layout:?}"
    );
  }

  #[test]
  fn bibliography_paragraph_keeps_generated_styling() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let (layout, _headings) = lower_bibliography(&LoweringContext::new(&style), &bibliography(), 0);

    // Assert
    let italic = layout.iter().find_map(|n| match n {
      LayoutNode::Text(t, s) if t == "Crazy Rich Asians" => return Some(*s),
      _ => return None,
    });
    assert_eq!(italic.map(|s| return s.font_kind), Some(crate::model::FontKind::SerifItalic), "{layout:?}");
  }

  #[test]
  fn generated_internal_link_maps_to_citation_anchor() {
    // Arrange — `\cite` の表示は生成物なので、この経路で lower される
    let style = ReadStyle::default();
    let analyzed = analyzed("\\cite{kwan2014}\n");
    let (site, _) = analyzed.citation_sites().iter().next().expect("引用箇所が 1 件あるはず");
    let mut displays: NodeMap<Vec<InlineNode>> = NodeMap::default();
    displays.insert(
      site,
      vec![InlineNode::InternalLink {
        target: CitationId::new("kwan2014"),
        children: vec![InlineNode::Text("[1]".to_string())],
      }],
    );

    // Act
    let layout = lower(&style, &analyzed, &displays, &[]);

    // Assert
    let LayoutNode::Link { target, children } = &layout[0] else {
      panic!("Link が期待されます: {layout:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(CitationId::new("kwan2014"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "[1]"), "{children:?}");
  }
}
