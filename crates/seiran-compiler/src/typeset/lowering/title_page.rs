//! タイトルページ（`\maketitle` 相当）の lowering

use crate::{
  document::{TextAlignment, Typeface},
  length::Length,
  style::TitlePageStyle,
  typeset::lowering::layout_node::{InlineNode, LayoutNode, ParagraphLayout, TextStyle},
};

/// タイトルページに載せる文書メタデータ。
#[derive(Debug, Clone, Default)]
pub(crate) struct TitlePageMetadata {
  /// タイトル（`[document] title`）
  pub title: Option<String>,
  /// 著者（`[document] author`）
  pub author: Option<String>,
  /// 日付（`[document] date`）
  pub date: Option<String>,
}

/// タイトルページのレイアウトノード列を生成する。
///
/// 中身（タイトル・著者・日付のいずれか）があれば中央寄せの縦ボックスと末尾の改ページを返し、
/// 無ければ空を返す（前付けに白紙ページを作らない）。
#[must_use]
pub(crate) fn lower_title_page(meta: &TitlePageMetadata, style: &TitlePageStyle) -> Vec<LayoutNode> {
  let mut body: Vec<LayoutNode> = Vec::new();

  let entries: [(Option<&str>, Length, Typeface, Length); 3] = [
    (meta.title.as_deref(), style.title_font_size, style.title_typeface, style.title_bottom_margin),
    (meta.author.as_deref(), style.author_font_size, style.author_typeface, style.author_bottom_margin),
    (meta.date.as_deref(), style.date_font_size, style.date_typeface, Length::pt(0.0)),
  ];

  // 間隔 0 の隣接項目は同じ段落を共有する。最後の項目の下マージンは出力しない。
  let mut pending_gap = Length::ZERO;
  for (text, font_size, typeface, gap_after) in entries {
    let Some(text) = text.map(str::trim).filter(|trimmed| return !trimmed.is_empty()) else {
      continue;
    };
    let inline = InlineNode::Text(
      text.to_string(),
      TextStyle {
        font_size,
        typeface,
        color: None,
        script_level: None,
        math_operator: None,
      },
    );
    if let Some(LayoutNode::Paragraph(paragraph)) = body.last_mut() {
      if !pending_gap.is_positive() {
        paragraph.content.push(inline);
        pending_gap = gap_after;
        continue;
      }
      paragraph.spacing_after = pending_gap;
    }
    body.push(LayoutNode::Paragraph(ParagraphLayout {
      origin: None,
      content: vec![inline],
      spacing_after: Length::ZERO,
    }));
    pending_gap = gap_after;
  }

  if body.is_empty() {
    return Vec::new();
  }
  let mut children: Vec<LayoutNode> = Vec::with_capacity(body.len() + 1);
  if style.top_margin.is_positive() {
    children.push(LayoutNode::Vkern {
      length: style.top_margin,
    });
  }
  children.extend(body);
  return vec![
    LayoutNode::VBox {
      children,
      margin_bottom: Length::pt(0.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: Some(TextAlignment::Center),
    },
    LayoutNode::PageBreak,
  ];
}

#[cfg(test)]
mod tests {
  use super::{TitlePageMetadata, lower_title_page};
  use crate::{
    document::TextAlignment,
    length::Length,
    style::TitlePageStyle,
    typeset::lowering::{
      layout_node::{InlineNode, LayoutNode},
      test_support::{inlines, paragraph_layout, paragraph_origins},
    },
  };

  /// 中央寄せ `VBox` の子ノードを取り出すヘルパ
  fn title_vbox_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    for node in nodes {
      if let LayoutNode::VBox {
        children,
        alignment,
        ..
      } = node
      {
        assert_eq!(*alignment, Some(TextAlignment::Center), "タイトルページの VBox は中央寄せ");
        return children;
      }
    }
    panic!("VBox が見つからない: {nodes:?}");
  }

  /// 子ノードから Text 文字列だけを文書順に集めるヘルパ
  fn texts(children: &[LayoutNode]) -> Vec<String> {
    return inlines(children)
      .filter_map(|n| match n {
        InlineNode::Text(text, _) => return Some(text.clone()),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn full_metadata_yields_centered_vbox_then_page_break() {
    let meta = TitlePageMetadata {
      title: Some("My Title".to_string()),
      author: Some("Me".to_string()),
      date: Some("2026-06-15".to_string()),
    };
    let style = TitlePageStyle::default();

    let nodes = lower_title_page(&meta, &style);

    assert!(matches!(nodes.last(), Some(LayoutNode::PageBreak)), "末尾は PageBreak: {nodes:?}");
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["My Title", "Me", "2026-06-15"]);
  }

  #[test]
  fn title_uses_style_font_size_and_kind() {
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      ..TitlePageMetadata::default()
    };
    let style = TitlePageStyle {
      title_font_size: Length::pt(40.0),
      ..TitlePageStyle::default()
    };

    let nodes = lower_title_page(&meta, &style);

    let children = title_vbox_children(&nodes);
    let text_style = inlines(children)
      .find_map(|n| match n {
        InlineNode::Text(t, s) if t == "T" => return Some(*s),
        _ => return None,
      })
      .expect("Text が見つからない");
    assert_eq!(text_style.font_size, Length::pt(40.0));
    assert_eq!(text_style.typeface, style.title_typeface);
  }

  #[test]
  fn missing_author_skips_element_and_its_gap() {
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      author: None,
      date: Some("D".to_string()),
    };
    let style = TitlePageStyle::default();

    let nodes = lower_title_page(&meta, &style);

    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["T", "D"]);
    let vkern_count = children.iter().filter(|n| matches!(n, LayoutNode::Vkern { .. })).count();
    assert_eq!(vkern_count, 1, "top_margin だけが Vkern: {children:?}");
    assert_eq!(paragraph_layout(&children[1]).spacing_after, style.title_bottom_margin);
    assert_eq!(paragraph_layout(&children[2]).spacing_after, Length::ZERO);
  }

  #[test]
  fn empty_metadata_yields_nothing() {
    let meta = TitlePageMetadata::default();
    let style = TitlePageStyle::default();

    let nodes = lower_title_page(&meta, &style);

    assert!(nodes.is_empty(), "{nodes:?}");
  }

  #[test]
  fn blank_title_is_treated_as_empty() {
    let meta = TitlePageMetadata {
      title: Some("   ".to_string()),
      author: Some("A".to_string()),
      ..TitlePageMetadata::default()
    };
    let style = TitlePageStyle::default();

    let nodes = lower_title_page(&meta, &style);

    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["A"]);
  }

  #[test]
  fn title_page_paragraphs_have_no_origin() {
    // タイトルページの行は config.toml の文書メタデータ由来で、ソース要素に対応しない
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      author: Some("A".to_string()),
      date: Some("D".to_string()),
    };

    let nodes = lower_title_page(&meta, &TitlePageStyle::default());

    let origins = paragraph_origins(&nodes);
    assert_eq!(origins.len(), 3, "{nodes:?}");
    assert!(origins.iter().all(Option::is_none), "{origins:?}");
  }
}
