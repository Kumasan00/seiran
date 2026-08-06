//! CSL 整形の生成物（書誌・引用表示）の lowering
//!
//! 生成物は HIR ではない（`NodeId` を持たない）ので、著者が書いた本文とは別経路で lower する。
//! 箱組み（見出し・段落）そのものは本文と同じ関数を通し、この module が持つのは
//! 「`GeneratedInline` 列 → `LayoutNode` 列」の変換と、書誌に現れる固定形の走査だけ。

use super::{
  HeadingRecord, LoweringContext,
  heading::{self, title_style},
  layout_node::{LayoutNode, TextStyle},
  paragraph::{assemble_paragraph, body_text_style},
};
use crate::model::{
  AnchorId, AnchorMark, GeneratedBlock, GeneratedInline, HeadingKey, LinkTarget, generated_inlines_to_plain_text,
};

/// 書誌（CSL 整形の生成物）をレイアウトノードと見出し記録へ変換する
///
/// 書誌の見出しは無採番で、本文の続きとなる `HeadingKey` を `next_heading_index` から振る。
/// `GeneratedBlock` は生成物専用に絞られている（#325）ので、この match は網羅的で済む。
pub(super) fn lower_bibliography(
  ctx: &LoweringContext,
  nodes: &[GeneratedBlock],
  next_heading_index: usize,
) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
  let mut layout = Vec::with_capacity(nodes.len());
  let mut headings = Vec::new();
  let mut heading_index = next_heading_index;

  for node in nodes {
    match node {
      GeneratedBlock::Heading { level, title } => {
        let key = HeadingKey::new(heading_index);
        heading_index += 1;
        let style = title_style(ctx, *level);
        // 生成物の lowering には副作用がないので、遅延させても結果は変わらない。
        // 書誌の見出しは無採番（`citation::render` が合成する見出しに番号は無い）なので番号は空。
        layout.extend(heading::lower_heading(
          ctx,
          *level,
          "",
          || return lower_generated_inlines(ctx, title, style),
          None,
          key,
        ));
        headings.push(HeadingRecord {
          index: key.index(),
          level: *level,
          number: String::new(),
          title_plain: generated_inlines_to_plain_text(title),
        });
      },
      GeneratedBlock::Paragraph(inlines) => {
        let content = lower_generated_inlines(ctx, inlines, body_text_style(ctx));
        layout.extend(assemble_paragraph(ctx, content, false));
      },
      GeneratedBlock::Anchor(target) => layout.push(LayoutNode::Anchor(AnchorMark::Citation(target.clone()))),
    }
  }

  return (layout, headings);
}

/// 生成物のインライン列（CSL 整形の出力）をレイアウトノードへ変換する
///
/// 生成物には `\ref` も `\cite` も索引も脚注も現れない（`GeneratedInline` はそもそもそれらの
/// variant を持たない、#325）ので、事実を引く必要がなく `LoweringState` を取らない。
pub(super) fn lower_generated_inlines(
  ctx: &LoweringContext,
  inlines: &[GeneratedInline],
  parent_style: TextStyle,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for inline in inlines {
    result.extend(lower_generated_inline(ctx, inline, parent_style));
  }
  return result;
}

/// 生成物のインライン 1 個をレイアウトノードへ変換する
///
/// `GeneratedInline` は `citation::render` が実際に構築する 3 variant に絞られている
/// （#325 / #326）ので、この match は網羅的で済む。
fn lower_generated_inline(ctx: &LoweringContext, inline: &GeneratedInline, parent_style: TextStyle) -> Vec<LayoutNode> {
  match inline {
    GeneratedInline::Text(text) => return vec![LayoutNode::Text(text.clone(), parent_style)],
    GeneratedInline::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
        color: parent_style.color,
      };
      return lower_generated_inlines(ctx, children, styled);
    },
    GeneratedInline::InternalLink { target, children } => {
      return vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Citation(target.clone())),
        children: lower_generated_inlines(ctx, children, parent_style),
      }];
    },
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
  fn bibliography() -> Vec<GeneratedBlock> {
    return vec![
      GeneratedBlock::Heading {
        level: crate::model::HeadingLevel::Section,
        title: vec![GeneratedInline::Text("References".to_string())],
      },
      GeneratedBlock::Anchor(CitationId::new("kwan2014")),
      GeneratedBlock::Paragraph(vec![
        GeneratedInline::Text("K. Kwan, ".to_string()),
        GeneratedInline::Styled {
          kind: crate::model::FontKind::SerifItalic,
          children: vec![GeneratedInline::Text("Crazy Rich Asians".to_string())],
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
    let mut displays: NodeMap<Vec<GeneratedInline>> = NodeMap::default();
    displays.insert(
      site,
      vec![GeneratedInline::InternalLink {
        target: CitationId::new("kwan2014"),
        children: vec![GeneratedInline::Text("[1]".to_string())],
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
