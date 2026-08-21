//! タイトルページ（`\maketitle` 相当）の lowering

use crate::{
  document::FontKind,
  length::Length,
  style::TitlePageStyle,
  typeset::{
    boxes::Align,
    lowering::layout_node::{LayoutNode, TextStyle},
  },
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
#[must_use]
pub(crate) fn lower_title_page(meta: &TitlePageMetadata, style: &TitlePageStyle) -> Vec<LayoutNode> {
  let mut body: Vec<LayoutNode> = Vec::new();

  let entries: [(Option<&str>, Length, FontKind, Length); 3] = [
    (meta.title.as_deref(), style.title_font_size, style.title_font_kind, style.title_bottom_margin),
    (meta.author.as_deref(), style.author_font_size, style.author_font_kind, style.author_bottom_margin),
    (meta.date.as_deref(), style.date_font_size, style.date_font_kind, Length::pt(0.0)),
  ];

  // 直前に積んだ要素の下マージン。次の present な要素を積む直前に Vkern として挿入する。
  let mut pending_gap: Option<Length> = None;
  for (text, font_size, font_kind, gap_after) in entries {
    let Some(text) = text.map(str::trim).filter(|trimmed| return !trimmed.is_empty()) else {
      continue;
    };
    if let Some(gap) = pending_gap.take()
      && gap.is_positive()
    {
      body.push(LayoutNode::Vkern { length: gap });
    }
    body.push(LayoutNode::Text(
      text.to_string(),
      TextStyle {
        font_size,
        font_kind,
        color: None,
      },
    ));
    pending_gap = Some(gap_after);
  }

  let mut result: Vec<LayoutNode> = Vec::with_capacity(2);
  if !body.is_empty() {
    let mut children: Vec<LayoutNode> = Vec::with_capacity(body.len() + 1);
    if style.top_margin.is_positive() {
      children.push(LayoutNode::Vkern {
        length: style.top_margin,
      });
    }
    children.extend(body);
    result.push(LayoutNode::VBox {
      children,
      margin_bottom: Length::pt(0.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    });
  }
  result.push(LayoutNode::PageBreak);
  return result;
}

#[cfg(test)]
mod tests {
  use super::{super::layout_node::LayoutNode, TitlePageMetadata, lower_title_page};
  use crate::{length::Length, style::TitlePageStyle, typeset::boxes::Align};

  /// 中央寄せ `VBox` の子ノードを取り出すヘルパ
  fn title_vbox_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    for node in nodes {
      if let LayoutNode::VBox {
        children, align, ..
      } = node
      {
        assert_eq!(*align, Align::Center, "タイトルページの VBox は中央寄せ");
        return children;
      }
    }
    panic!("VBox が見つからない: {nodes:?}");
  }

  /// 子ノードから Text 文字列だけを文書順に集めるヘルパ
  fn texts(children: &[LayoutNode]) -> Vec<String> {
    return children
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(text, _) => return Some(text.clone()),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn full_metadata_yields_centered_vbox_then_page_break() {
    // Arrange
    let meta = TitlePageMetadata {
      title: Some("My Title".to_string()),
      author: Some("Me".to_string()),
      date: Some("2026-06-15".to_string()),
    };
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    assert!(matches!(nodes.last(), Some(LayoutNode::PageBreak)), "末尾は PageBreak: {nodes:?}");
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["My Title", "Me", "2026-06-15"]);
  }

  #[test]
  fn title_uses_style_font_size_and_kind() {
    // Arrange
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      ..TitlePageMetadata::default()
    };
    let style = TitlePageStyle {
      title_font_size: Length::pt(40.0),
      ..TitlePageStyle::default()
    };

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    let children = title_vbox_children(&nodes);
    let text_style = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "T" => return Some(*s),
        _ => return None,
      })
      .expect("Text が見つからない");
    assert_eq!(text_style.font_size, Length::pt(40.0));
    assert_eq!(text_style.font_kind, style.title_font_kind);
  }

  #[test]
  fn missing_author_skips_element_and_its_gap() {
    // Arrange
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      author: None,
      date: Some("D".to_string()),
    };
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["T", "D"]);
    let vkern_count = children.iter().filter(|n| matches!(n, LayoutNode::Vkern { .. })).count();
    assert_eq!(vkern_count, 2, "top_margin と要素間アキで Vkern は 2 つ: {children:?}");
  }

  #[test]
  fn empty_metadata_yields_only_page_break() {
    // Arrange
    let meta = TitlePageMetadata::default();
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0], LayoutNode::PageBreak));
  }

  #[test]
  fn blank_title_is_treated_as_empty() {
    // Arrange
    let meta = TitlePageMetadata {
      title: Some("   ".to_string()),
      author: Some("A".to_string()),
      ..TitlePageMetadata::default()
    };
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["A"]);
  }
}
