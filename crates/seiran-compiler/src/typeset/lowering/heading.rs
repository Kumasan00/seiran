//! 見出し（`document::HirNodeKind::Heading` と、CSL 整形が合成する書誌見出し）の lowering

use super::{
  LoweringContext,
  layout_node::{LayoutNode, TextStyle},
  template::expand_template,
};
use crate::{
  document::HeadingLevel,
  semantics::{HeadingKey, LabelId},
  typeset::boxes::{Align, AnchorMark},
};

/// 見出しのタイトル・番号に使う基底テキストスタイルを返す
///
/// タイトルの lowering は呼び出し元（本文なら HIR、書誌なら CSL 整形の生成物）が行うため、
/// そこで使うスタイルをこの 1 箇所から配る。
pub(super) fn title_style(ctx: &LoweringContext, level: HeadingLevel) -> TextStyle {
  let heading_style = ctx.style.heading(level);
  return TextStyle {
    font_size: heading_style.font_size,
    font_kind: heading_style.font_kind,
    color: None,
  };
}

/// 見出しをレイアウトノードに変換する
///
/// `title` は「呼ぶとタイトルを lower して返すクロージャ」。`format` に `{title}` が現れた
/// ときだけ、現れた回数ぶん呼ばれる（タイトル中の `\footnote` が採番だけ消費する事故を防ぐ。
/// 詳細は [`expand_template`] の doc コメント）。
pub(super) fn lower_heading(
  ctx: &LoweringContext,
  level: HeadingLevel,
  number: &str,
  title: impl FnMut() -> Vec<LayoutNode>,
  label: Option<LabelId>,
  key: HeadingKey,
) -> Vec<LayoutNode> {
  let heading_style = ctx.style.heading(level);
  let style = title_style(ctx, level);

  let children = expand_template(&heading_style.format, number, title, None, style);

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  // しおり・目次リンク・`\ref` の到達先アンカー。改ページ後に置くことで正しいページに解決される。
  // `key` は `analyze` が文書順に振ったもの（目次エントリの内部リンクと一致する）。
  result.push(LayoutNode::Anchor(AnchorMark::Heading { key, label }));

  result.push(LayoutNode::VBox {
    children,
    margin_bottom: heading_style.bottom_margin,
    indent: crate::length::Length::pt(0.0),
    right_indent: crate::length::Length::pt(0.0),
    align: Align::Left,
  });

  // 見出し直後の改ページ制御。強制改ページ（page_break_after）と keep-with-next は排他:
  // page_break_after の見出し（Part 等）は意図的にページを終えるため keep-with-next を課さない。
  // それ以外の見出しは直後のブロックとの分割を禁止し、見出しがページ末尾に孤立するのを防ぐ。
  if heading_style.page_break_after {
    result.push(LayoutNode::PageBreak);
  } else {
    result.push(LayoutNode::KeepWithNext);
  }

  return result;
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
    font::FontKind,
    typeset::boxes::{AnchorId, LinkTarget},
  };

  /// 基底スタイルのプレーンなタイトルノード 1 個を作る
  fn plain_title(ctx: &LoweringContext, level: HeadingLevel, text: &str) -> Vec<LayoutNode> {
    return vec![LayoutNode::Text(text.to_string(), title_style(ctx, level))];
  }

  /// `nodes` から見出し `VBox` の子要素列を取り出す
  fn heading_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children.as_slice()),
        _ => return None,
      })
      .expect("VBox が出力されるはず");
  }

  #[test]
  fn lower_heading_uses_style_template() {
    // Arrange
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Custom Title");

    // Act
    let nodes = lower_heading(&ctx, HeadingLevel::Section, "4.7", || return title.clone(), None, HeadingKey::new(0));

    // Assert
    let children = heading_children(&nodes);
    let text = match &children[0] {
      LayoutNode::Text(text, _) => text.clone(),
      other => panic!("Text ノードが期待されます: {other:?}"),
    };
    assert_eq!(text, "[4.7] Custom Title");
  }

  #[test]
  fn lower_heading_preserves_styled_title() {
    // Arrange — 書体切り替えを含むタイトルは呼び出し元が lower して渡す
    let style = ReadStyle::default();

    // Act
    let nodes = lower(&style, &analyzed("\\section{Intro \\italic{Italic}}\n"));

    // Assert
    let children = heading_children(&nodes);
    let heading_size = style.heading(HeadingLevel::Section).font_size;
    let italic = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "Italic" => return Some(*s),
        _ => return None,
      })
      .expect("イタリック部分の Text があるはず");
    assert_eq!(italic.font_kind, FontKind::SerifItalic);
    assert_eq!(italic.font_size, heading_size, "フォントサイズは見出しスタイルを継承する");
  }

  #[test]
  fn lower_heading_emits_anchor_with_label() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    // Act
    let nodes = lower_heading(
      &ctx,
      HeadingLevel::Section,
      "1",
      || return title.clone(),
      Some(LabelId::new("sec:intro")),
      HeadingKey::new(3),
    );

    // Assert
    let anchor = nodes.iter().find_map(|n| match n {
      LayoutNode::Anchor(mark) => return Some(mark.clone()),
      _ => return None,
    });
    assert_eq!(
      anchor,
      Some(AnchorMark::Heading {
        key: HeadingKey::new(3),
        label: Some(LabelId::new("sec:intro")),
      })
    );
    let anchor_idx = nodes.iter().position(|n| matches!(n, LayoutNode::Anchor(_))).unwrap();
    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    assert!(anchor_idx < vbox_idx, "アンカーは VBox より前: {nodes:?}");
  }

  #[test]
  fn lower_heading_emits_keep_with_next_after_vbox() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    // Act
    let nodes = lower_heading(&ctx, HeadingLevel::Section, "1", || return title.clone(), None, HeadingKey::new(0));

    // Assert
    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    let keep_idx = nodes.iter().position(|n| matches!(n, LayoutNode::KeepWithNext)).expect("KeepWithNext が出るはず");
    assert!(keep_idx > vbox_idx, "KeepWithNext は VBox の後に出る: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "改ページは出ない: {nodes:?}");
  }

  #[test]
  fn lower_heading_with_page_break_after_omits_keep_with_next() {
    // Arrange
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].page_break_after = true;
    let ctx = LoweringContext::new(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    // Act
    let nodes = lower_heading(&ctx, HeadingLevel::Section, "1", || return title.clone(), None, HeadingKey::new(0));

    // Assert
    assert!(nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "強制改ページが出るはず: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::KeepWithNext)), "KeepWithNext は出ない: {nodes:?}");
  }

  /// レイアウトノード列の最上位から脚注の (表示番号, 通し index) を文書順に集める
  fn footnotes(nodes: &[LayoutNode]) -> Vec<(u32, u32)> {
    return nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Footnote { number, index, .. } => return Some((*number, *index)),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn heading_format_without_title_placeholder_does_not_consume_footnote_number() {
    // Arrange — `{title}` を含まない独自フォーマット（タイトルは一切表示されない）
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].format = "{number}".to_string();

    // Act
    let nodes = lower(&style, &analyzed("\\section{Intro\\footnote{in title}}\n\nbody\\footnote{in body}\n"));

    // Assert — タイトルを lower しないので、本文の脚注が 1 番のままになる
    assert_eq!(footnotes(&nodes), vec![(1, 0)], "{nodes:?}");
  }

  #[test]
  fn heading_format_with_two_title_placeholders_lowers_title_twice() {
    // Arrange — `{title}` を 2 回含むフォーマット
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].format = "{title} / {title}".to_string();

    // Act
    let nodes = lower(&style, &analyzed("\\section{Intro\\footnote{n}}\n"));

    // Assert — 出現ごとに lower し直すので、マーカーと本体が対になった別々の脚注が 2 個出る
    assert_eq!(footnotes(heading_children(&nodes)), vec![(1, 0), (2, 1)], "{nodes:?}");
  }

  #[test]
  fn ref_in_heading_title_is_resolved_to_internal_link() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let nodes = lower(&style, &analyzed("\\chapter[label=ch:other]{Other}\n\n\\section{\\ref{ch:other}}\n"));

    // Assert — 2 つ目の見出し（section）の VBox に解決済みリンクが入る
    let children = nodes
      .iter()
      .rev()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(children.as_slice()),
        _ => return None,
      })
      .expect("section の VBox があるはず");
    let link = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Link { target, children } => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, LinkTarget::Internal(AnchorId::Label(LabelId::new("ch:other"))));
    assert!(matches!(&link.1[0], LayoutNode::Text(t, _) if t == "Chapter 1"), "{:?}", link.1);
  }
}
