//! CSL 整形の生成物（書誌・引用表示）の lowering
//!
//! 生成物は HIR ではない（`NodeId` を持たない）ので、著者が書いた本文とは別経路で lower する。
//! 箱組み（見出し・段落）そのものは本文と同じ関数を通し、この module が持つのは
//! 「`GeneratedInline` 列 → `LayoutNode` 列」の変換と、書誌の見出し（style 由来）＋
//! エントリ列の組み立てだけ。

use crate::{
  document::HeadingLevel,
  semantics::{BibliographyEntry, GeneratedInline, HeadingKey},
  typeset::{
    boxes::{AnchorId, AnchorMark, LinkTarget},
    lowering::{
      HeadingRecord, LoweringContext,
      heading::{self, title_style},
      layout_node::{LayoutNode, TextStyle},
      paragraph::{assemble_paragraph, body_text_style},
    },
  },
};

/// 書誌見出しのレベル
///
/// `citation::render` が見出しを合成していた頃から `Section` 固定で、style に選択肢は無い。
const BIBLIOGRAPHY_HEADING_LEVEL: HeadingLevel = HeadingLevel::Section;

/// 書誌（CSL 整形の生成物）をレイアウトノードと見出し記録へ変換する
///
/// 書誌見出しは style の値（`style.reference.title`）から作る — semantics の成果物には
/// 見出しが無く、エントリ列だけが来る（#667）。見出しは無採番で、本文の続きとなる
/// `HeadingKey` を `next_heading_index` から振る。`bibliography` が `None`（CSL が書誌を
/// 定義していない）のときは見出しも出さない。
pub(super) fn lower_bibliography(
  ctx: &LoweringContext<'_>,
  bibliography: Option<&[BibliographyEntry]>,
  next_heading_index: usize,
) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
  let Some(entries) = bibliography else {
    return (Vec::new(), Vec::new());
  };

  let key = HeadingKey::new(next_heading_index);
  let title = vec![GeneratedInline::Text(ctx.style.reference.title.clone())];
  let style = title_style(ctx, BIBLIOGRAPHY_HEADING_LEVEL);
  // 生成物の lowering には副作用がないので、遅延させても結果は変わらない。
  // 書誌の見出しは無採番（style に番号書式を持たない）なので番号は空。
  let mut layout = heading::lower_heading(
    ctx,
    BIBLIOGRAPHY_HEADING_LEVEL,
    "",
    || return lower_generated_inlines(ctx, &title, style),
    None,
    key,
  );
  let headings = vec![HeadingRecord {
    index: key.index(),
    level: BIBLIOGRAPHY_HEADING_LEVEL,
    number: String::new(),
    title_plain: ctx.style.reference.title.clone(),
  }];

  for entry in entries {
    layout.push(LayoutNode::Anchor(AnchorMark::Citation(entry.key.clone())));
    let content = lower_generated_inlines(ctx, &entry.body, body_text_style(ctx));
    layout.extend(assemble_paragraph(ctx, content, false));
  }

  return (layout, headings);
}

/// 生成物のインライン列（CSL 整形の出力）をレイアウトノードへ変換する
///
/// 生成物には `\ref` も `\cite` も索引も脚注も現れない（`GeneratedInline` はそもそもそれらの
/// variant を持たない、#325）ので、事実を引く必要がなく `LoweringState` を取らない。
pub(super) fn lower_generated_inlines(
  ctx: &LoweringContext<'_>,
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
fn lower_generated_inline(
  ctx: &LoweringContext<'_>,
  inline: &GeneratedInline,
  parent_style: TextStyle,
) -> Vec<LayoutNode> {
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
mod tests {
  use super::*;
  use crate::{
    document::FontKind,
    semantics::CitationId,
    style::Style as ReadStyle,
    typeset::lowering::{
      lower_sources_with_headings,
      test_support::{analyzed, lower},
    },
  };

  /// `citation::render` が作るのと同じ形の書誌エントリ列（1 件）を作る
  fn bibliography() -> Vec<BibliographyEntry> {
    return vec![BibliographyEntry {
      key: CitationId::new("kwan2014"),
      body: vec![
        GeneratedInline::Text("K. Kwan, ".to_string()),
        GeneratedInline::Styled {
          kind: FontKind::SerifItalic,
          children: vec![GeneratedInline::Text("Crazy Rich Asians".to_string())],
        },
      ],
    }];
  }

  #[test]
  fn bibliography_is_appended_after_body_with_continuing_heading_key() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed("\\section{本文}\n");
    let ctx = LoweringContext::new(&style);

    // Act
    let document = analyzed.with_citations_for_test(Vec::new(), Some(bibliography()));
    let (layout, headings) = lower_sources_with_headings(&ctx, &document);

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
    let (layout, _headings) = lower_bibliography(&LoweringContext::new(&style), Some(&bibliography()), 0);

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
    let (layout, _headings) = lower_bibliography(&LoweringContext::new(&style), Some(&bibliography()), 0);

    // Assert
    let italic = layout.iter().find_map(|n| match n {
      LayoutNode::Text(t, s) if t == "Crazy Rich Asians" => return Some(*s),
      _ => return None,
    });
    assert_eq!(italic.map(|s| return s.font_kind), Some(FontKind::SerifItalic), "{layout:?}");
  }

  #[test]
  fn generated_internal_link_maps_to_citation_anchor() {
    // Arrange — `\cite` の表示は生成物なので、この経路で lower される
    let style = ReadStyle::default();
    let analyzed = analyzed("\\cite{kwan2014}\n");
    let site = analyzed.citation_sites().next().expect("引用箇所が 1 件あるはず");
    let document = analyzed.with_citations_for_test(
      vec![(
        site,
        vec![GeneratedInline::InternalLink {
          target: CitationId::new("kwan2014"),
          children: vec![GeneratedInline::Text("[1]".to_string())],
        }],
      )],
      None,
    );

    // Act
    let layout = lower(&style, &document);

    // Assert
    let LayoutNode::Link { target, children } = &layout[0] else {
      panic!("Link が期待されます: {layout:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(CitationId::new("kwan2014"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "[1]"), "{children:?}");
  }

  #[test]
  fn empty_bibliography_still_emits_heading() {
    // Arrange — CSL に書誌があってエントリが 0 件の状態
    let style = ReadStyle::default();

    // Act
    let (layout, headings) = lower_bibliography(&LoweringContext::new(&style), Some(&[]), 0);

    // Assert — 見出しは 1 件出るが、エントリ由来のアンカーは無い
    assert_eq!(headings.len(), 1, "エントリ 0 件でも書誌見出しは出るはず: {headings:?}");
    assert_eq!(headings[0].title_plain, "References");
    assert!(
      !layout.iter().any(|n| matches!(n, LayoutNode::Anchor(AnchorMark::Citation(_)))),
      "エントリが無ければ引用アンカーも無いはず: {layout:?}"
    );
  }

  #[test]
  fn absent_bibliography_emits_nothing() {
    // Arrange — CSL が書誌を定義していない状態
    let style = ReadStyle::default();

    // Act
    let (layout, headings) = lower_bibliography(&LoweringContext::new(&style), None, 0);

    // Assert
    assert!(layout.is_empty(), "書誌が無ければレイアウトノードは出ないはず: {layout:?}");
    assert!(headings.is_empty(), "書誌が無ければ見出し記録も出ないはず: {headings:?}");
  }

  #[test]
  fn bibliography_heading_title_comes_from_style() {
    // Arrange — 書誌見出しの文字列は style の値（生成物には埋め込まれていない）
    let mut style = ReadStyle::default();
    style.reference.title = "参考文献".to_string();

    // Act
    let (_layout, headings) = lower_bibliography(&LoweringContext::new(&style), Some(&bibliography()), 0);

    // Assert
    assert_eq!(headings[0].title_plain, "参考文献", "style.reference.title が見出しになるはず");
  }
}
