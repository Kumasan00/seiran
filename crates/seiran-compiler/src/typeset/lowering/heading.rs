//! 見出し（`document::HirNodeKind::Heading` と、CSL 整形が合成する書誌見出し）の lowering

use crate::{
  document::{HeadingLevel, HirHeading, HirInline, HirInlineKind, NodeId},
  length::Length,
  semantics::{HeadingKey, LabelId, generated_inlines_to_plain_text},
  style::Style as ReadStyle,
  typeset::{
    boxes::{Align, AnchorId},
    lowering::{
      LoweringContext, LoweringState, counter, inline,
      layout_node::{InlineNode, LayoutNode, TextStyle, merge_adjacent_text},
    },
  },
};

/// 見出しのタイトル・番号に使う基底テキストスタイルを返す
///
/// タイトルの lowering は呼び出し元（本文なら HIR、書誌なら CSL 整形の生成物）が行うため、
/// そこで使うスタイルをこの 1 箇所から配る。
pub(super) fn title_style(ctx: &LoweringContext<'_>, level: HeadingLevel) -> TextStyle {
  let heading_style = ctx.style.heading(level);
  return TextStyle {
    font_size: heading_style.font_size,
    font_kind: heading_style.font_kind,
    color: None,
  };
}

/// HIR のインライン列をプレーンテキストへ畳む（見出しタイトルのしおり・目次表示用）
///
/// `GeneratedInline` 側のプレーンテキスト畳み込み（`semantics` の `generated_inlines_to_plain_text`）と
/// 同じ規則を保つ。バリアントごとの扱い（数式は `"[Math]"`、脚注・索引は空、`\cite` は整形済み表示を
/// 辿る等）は同じに保つ。
fn hir_inlines_to_plain_text(inlines: &[HirInline], style: &ReadStyle, state: &LoweringState<'_>) -> String {
  let mut out = String::new();
  for inline in inlines {
    match &inline.kind {
      HirInlineKind::Text(s) => out.push_str(s),
      HirInlineKind::Styled { children, .. }
      | HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. } => {
        out.push_str(&hir_inlines_to_plain_text(children, style, state));
      },
      // 引用の表示は生成物の side table にある（見出しの `\cite` も目次・しおりでは表示を辿る）。
      // 生成物は `GeneratedInline` なので生成物側の畳み込みをそのまま使う。
      HirInlineKind::Cite { .. } => {
        out.push_str(&generated_inlines_to_plain_text(state.citation_display(inline.id)));
      },
      HirInlineKind::Code(text) => out.push_str(text),
      HirInlineKind::InlineMath(_) => out.push_str("[Math]"),
      HirInlineKind::Symbol(ch) => out.push(*ch),
      HirInlineKind::LineBreak => out.push('\n'),
      // 脚注本体・索引マーカーは見出しのプレーンテキスト抽出には含めない（NoIndent と同じ空扱い）
      HirInlineKind::NoIndent | HirInlineKind::Footnote { .. } | HirInlineKind::Index { .. } => {},
      HirInlineKind::Ref { .. } => out.push_str(&state.ref_display(style, state.reference_target(inline.id))),
    }
  }
  return out;
}

/// HIR の見出しノードをレイアウトノードに変換する
///
/// 見出しキーは `semantics::analyze` が文書順に振ったもの。lowering は振り直さず読むだけなので、
/// 再帰（quote / theorem / list item 本体）を挟んでも `analyzed.headings()` の添字と必ず揃う。
pub(super) fn lower_hir_heading(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  heading: &HirHeading,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let key = state.heading_key(id);
  let label = state.declared_label(id).cloned();
  let number = state
    .counter_value(id)
    .map_or_else(String::new, |value| return counter::format_counter_value(ctx.style, value));
  // プレーンテキスト（しおり・目次表示）は不変借用でしか作れないので、可変借用が要る
  // タイトルの lowering より先に済ませる。
  let plain = hir_inlines_to_plain_text(&heading.title, ctx.style, &*state);
  state.record_heading_title(id, plain);
  let style = title_style(ctx, heading.level);
  // タイトルの lowering はクロージャで遅延させる。`heading.format` が `{title}` を含まない
  // なら一度も呼ばれず、タイトル中の `\footnote` が通し index だけ消費して消える事故を防ぐ。
  return lower_heading(
    ctx,
    heading.level,
    &number,
    || return inline::lower_inlines(ctx, &heading.title, style, state),
    label,
    key,
  );
}

/// 見出しをレイアウトノードに変換する
///
/// `title` は「呼ぶとタイトルを lower して返すクロージャ」。`format` に `{title}` が現れた
/// ときだけ、現れた回数ぶん呼ばれる（タイトル中の `\footnote` が採番だけ消費する事故を防ぐ。
/// 詳細は [`crate::style::NumberTitleTemplate::expand`] の doc コメント）。
pub(super) fn lower_heading(
  ctx: &LoweringContext<'_>,
  level: HeadingLevel,
  number: &str,
  title: impl FnMut() -> Vec<InlineNode>,
  label: Option<LabelId>,
  key: HeadingKey,
) -> Vec<LayoutNode> {
  let heading_style = ctx.style.heading(level);
  let style = title_style(ctx, level);

  let children = heading_style.format.expand(number, title, |literal| {
    return InlineNode::Text(literal.to_string(), style);
  });
  let children: Vec<LayoutNode> = merge_adjacent_text(children).into_iter().map(LayoutNode::from).collect();

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  // しおり・目次リンク・`\ref` の到達先アンカー。改ページ後に置くことで正しいページに解決される。
  // `key` は `analyze` が文書順に振ったもの（目次エントリの内部リンクと一致する）。ラベルのアンカーは
  // 直後に続けて置き、次の実ブロックの同じ座標で解決される。
  result.push(LayoutNode::Anchor(AnchorId::Heading(key)));
  if let Some(label) = label {
    result.push(LayoutNode::Anchor(AnchorId::Label(label)));
  }

  result.push(LayoutNode::VBox {
    children,
    margin_bottom: heading_style.bottom_margin,
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
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
mod tests {
  use super::*;
  use crate::{
    document::FontKind,
    style::{NumberTitleTemplate, Style as ReadStyle},
    typeset::{
      boxes::{AnchorId, LinkTarget},
      lowering::test_support::{analyzed, context, lower},
    },
  };

  /// 基底スタイルのプレーンなタイトルノード 1 個を作る
  fn plain_title(ctx: &LoweringContext<'_>, level: HeadingLevel, text: &str) -> Vec<InlineNode> {
    return vec![InlineNode::Text(text.to_string(), title_style(ctx, level))];
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
    style.heading.section.format = NumberTitleTemplate::parse("[{number}] {title}");
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Custom Title");

    // Act
    let nodes = lower_heading(&ctx, HeadingLevel::Section, "4.7", || return title.clone(), None, HeadingKey::new(0));

    // Assert
    let children = heading_children(&nodes);
    let text = match &children[0] {
      LayoutNode::Inline(InlineNode::Text(text, _)) => text.clone(),
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
        LayoutNode::Inline(InlineNode::Text(t, s)) if t == "Italic" => return Some(*s),
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
    let ctx = context(&style);
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

    // Assert — 見出しキーのアンカー、ラベルのアンカーの順で、どちらも VBox より前
    let anchors: Vec<(usize, &AnchorId)> = nodes
      .iter()
      .enumerate()
      .filter_map(|(i, n)| match n {
        LayoutNode::Anchor(id) => return Some((i, id)),
        _ => return None,
      })
      .collect();
    let ids: Vec<&AnchorId> = anchors.iter().map(|(_, id)| return *id).collect();
    assert_eq!(
      ids,
      vec![
        &AnchorId::Heading(HeadingKey::new(3)),
        &AnchorId::Label(LabelId::new("sec:intro"))
      ]
    );
    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    assert!(anchors.iter().all(|(i, _)| return *i < vbox_idx), "アンカーは VBox より前: {nodes:?}");
  }

  #[test]
  fn lower_heading_emits_keep_with_next_after_vbox() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = context(&style);
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
    style.heading.section.page_break_after = true;
    let ctx = context(&style);
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
        LayoutNode::Inline(InlineNode::Footnote { number, index, .. }) => return Some((*number, *index)),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn heading_format_without_title_placeholder_does_not_consume_footnote_number() {
    // Arrange — `{title}` を含まない独自フォーマット（タイトルは一切表示されない）
    let mut style = ReadStyle::default();
    style.heading.section.format = NumberTitleTemplate::parse("{number}");

    // Act
    let nodes = lower(&style, &analyzed("\\section{Intro\\footnote{in title}}\n\nbody\\footnote{in body}\n"));

    // Assert — タイトルを lower しないので、本文の脚注が 1 番のままになる
    assert_eq!(footnotes(&nodes), vec![(1, 0)], "{nodes:?}");
  }

  #[test]
  fn heading_format_with_two_title_placeholders_lowers_title_twice() {
    // Arrange — `{title}` を 2 回含むフォーマット
    let mut style = ReadStyle::default();
    style.heading.section.format = NumberTitleTemplate::parse("{title} / {title}");

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
        LayoutNode::Inline(InlineNode::Link { target, children }) => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, LinkTarget::Internal(AnchorId::Label(LabelId::new("ch:other"))));
    assert!(matches!(&link.1[0], InlineNode::Text(t, _) if t == "Chapter 1"), "{:?}", link.1);
  }
}
