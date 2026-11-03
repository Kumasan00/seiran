//! 見出し（`document::HirNodeKind::Heading` と、CSL 整形が合成する書誌見出し）の lowering

use crate::{
  document::{HeadingLevel, HirHeading, HirInline, HirInlineKind, NodeId, Typeface},
  length::Length,
  semantics::{GeneratedInline, HeadingKey, LabelId, generated_inlines_to_plain_text},
  style::Style as ReadStyle,
  typeset::{
    boxes::AnchorId,
    lowering::{
      LoweringContext, LoweringState, code, counter, inline,
      layout_node::{InlineNode, LayoutNode, ParagraphLayout, TextStyle, merge_adjacent_text},
      math::{lower_inline_math, unicode_math_text},
      quantity,
    },
  },
};

/// 見出しのタイトル・番号に使う基底テキストスタイルを返す
pub(super) fn title_style(ctx: &LoweringContext<'_>, level: HeadingLevel) -> TextStyle {
  let heading_style = &ctx.style.heading[level];
  return TextStyle {
    font_size: heading_style.font_size,
    typeface: heading_style.typeface,
    color: None,
    script_level: None,
    math_operator: None,
  };
}

/// 目次の行の題目の外側のスタイル（`[toc].font_size` の Serif・色なし）を返す
pub(in crate::typeset) fn toc_entry_style(style: &ReadStyle) -> TextStyle {
  return TextStyle {
    font_size: style.toc.font_size,
    typeface: Typeface::Serif,
    color: None,
    script_level: None,
    math_operator: None,
  };
}

/// 見出しの題目を、目次の行に組む写しのインライン列へ lower する
///
/// 本文と同じノード・書体規則で組む。`\ref` / `\cite` / `\href` / `\url` は表示だけを組んでリンクとリンク色を付けず、
/// 脚注・索引は写さず、改行は語間空白 1 個にする。`state` は共有参照なので、写しは脚注の採番を進めない。
fn toc_title(
  ctx: &LoweringContext<'_>,
  inlines: &[HirInline],
  parent_style: TextStyle,
  state: &LoweringState<'_>,
) -> Vec<InlineNode> {
  let mut out = Vec::new();
  for inline in inlines {
    match &inline.kind {
      HirInlineKind::Text(text) => out.push(InlineNode::Text(text.clone(), parent_style)),
      HirInlineKind::Symbol(ch) => out.push(InlineNode::Text(ch.to_string(), parent_style)),
      HirInlineKind::Styled { font, children } => {
        out.extend(toc_title(ctx, children, inline::styled(parent_style, *font), state));
      },
      HirInlineKind::Colored { color, children } => {
        out.extend(toc_title(ctx, children, inline::colored(parent_style, *color), state));
      },
      HirInlineKind::Link { children, .. } => out.extend(toc_title(ctx, children, parent_style, state)),
      HirInlineKind::Code(text) => out.extend(code::lower_inline_code(text, parent_style)),
      HirInlineKind::Math(math_nodes) => {
        out.extend(lower_inline_math(math_nodes, parent_style.font_size, parent_style.color, ctx.style));
      },
      HirInlineKind::Number(numeral) => {
        out.push(inline::quantity_node(ctx, quantity::number_pieces(numeral, &ctx.style.quantity), parent_style));
      },
      HirInlineKind::Unit(factors) => {
        out.push(inline::quantity_node(ctx, quantity::unit_pieces(factors, &ctx.style.quantity), parent_style));
      },
      HirInlineKind::Quantity { numeral, unit } => {
        let pieces = quantity::quantity_pieces(numeral, unit, &ctx.style.quantity);
        out.push(inline::quantity_node(ctx, pieces, parent_style));
      },
      HirInlineKind::Ref { .. } => {
        out.push(InlineNode::Text(state.ref_display(ctx.style, state.ref_target(inline.id)), parent_style));
      },
      HirInlineKind::Cite { .. } => out.extend(generated_toc_title(state.citation_display(inline.id), parent_style)),
      HirInlineKind::LineBreak => out.push(InlineNode::Text(" ".to_string(), parent_style)),
      HirInlineKind::Footnote { .. } | HirInlineKind::Index { .. } | HirInlineKind::NoIndent => {},
    }
  }
  return merge_adjacent_text(out);
}

/// 引用の表示（CSL 整形の生成物）を、目次の行に組む写しへ lower する（内部リンクは子だけを組む）
fn generated_toc_title(inlines: &[GeneratedInline], parent_style: TextStyle) -> Vec<InlineNode> {
  let mut out = Vec::new();
  for inline in inlines {
    match inline {
      GeneratedInline::Text(text) => out.push(InlineNode::Text(text.clone(), parent_style)),
      GeneratedInline::Styled { font, children } => {
        out.extend(generated_toc_title(children, inline::styled(parent_style, *font)));
      },
      GeneratedInline::InternalLink { children, .. } => out.extend(generated_toc_title(children, parent_style)),
    }
  }
  return out;
}

/// 目次の行に組む「番号 題目」を作る（結び方は [`crate::typeset::lowering::HeadingRecord::label`] と同じ）
fn toc_label(number: &str, title: Vec<InlineNode>, style: TextStyle) -> Vec<InlineNode> {
  if number.is_empty() {
    return title;
  }
  if title.is_empty() {
    return vec![InlineNode::Text(number.to_string(), style)];
  }
  let mut label = vec![InlineNode::Text(format!("{number} "), style)];
  label.extend(title);
  return merge_adjacent_text(label);
}

/// HIR のインライン列をプレーンテキストへ畳む（見出しタイトルのしおり用）
///
/// インライン数式は `UnicodeMath` の線形表記、改行は空白 1 個（しおりは 1 行）にする。両方にある種類（文字列・装飾・
/// リンク）は `GeneratedInline` 側の畳み込み（`semantics` の `generated_inlines_to_plain_text`）と同じ規則を保つ。
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
      HirInlineKind::Cite { .. } => {
        out.push_str(&generated_inlines_to_plain_text(state.citation_display(inline.id)));
      },
      HirInlineKind::Code(text) => out.push_str(text),
      HirInlineKind::Math(nodes) => out.push_str(&unicode_math_text(nodes, &style.quantity)),
      HirInlineKind::Symbol(ch) => out.push(*ch),
      HirInlineKind::LineBreak => out.push(' '),
      HirInlineKind::Number(numeral) => {
        out.push_str(&quantity::plain_text(&quantity::number_pieces(numeral, &style.quantity)));
      },
      HirInlineKind::Unit(factors) => {
        out.push_str(&quantity::plain_text(&quantity::unit_pieces(factors, &style.quantity)));
      },
      HirInlineKind::Quantity { numeral, unit } => {
        out.push_str(&quantity::plain_text(&quantity::quantity_pieces(numeral, unit, &style.quantity)));
      },
      HirInlineKind::NoIndent | HirInlineKind::Footnote { .. } | HirInlineKind::Index { .. } => {},
      HirInlineKind::Ref { .. } => out.push_str(&state.ref_display(style, state.ref_target(inline.id))),
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
  let plain = hir_inlines_to_plain_text(&heading.title, ctx.style, &*state);
  let entry_style = toc_entry_style(ctx.style);
  let toc = toc_label(&number, toc_title(ctx, &heading.title, entry_style, &*state), entry_style);
  state.record_heading_title(id, plain, toc);
  let style = title_style(ctx, heading.level);
  return lower_heading(
    ctx,
    heading.level,
    &number,
    || return inline::lower_inlines(ctx, &heading.title, style, state),
    label,
    key,
    Some(id),
  );
}

/// 見出しをレイアウトノードに変換する
///
/// `title` は呼ぶとタイトルを lower して返すクロージャで、`format` の `{title}` の出現回数だけ呼ばれる。
pub(super) fn lower_heading(
  ctx: &LoweringContext<'_>,
  level: HeadingLevel,
  number: &str,
  title: impl FnMut() -> Vec<InlineNode>,
  label: Option<LabelId>,
  key: HeadingKey,
  origin: Option<NodeId>,
) -> Vec<LayoutNode> {
  let heading_style = &ctx.style.heading[level];
  let style = title_style(ctx, level);

  let children = heading_style.format.expand(number, title, |literal| {
    return InlineNode::Text(literal.to_string(), style);
  });
  let children = vec![LayoutNode::Paragraph(ParagraphLayout {
    origin,
    content: merge_adjacent_text(children),
    spacing_after: Length::ZERO,
  })];

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  // しおり・目次リンク・`\ref` の到達先アンカー。改ページ後に置くことで正しいページに解決される。
  // ラベルのアンカーは直後に続けて置き、次の実ブロックの同じ座標で解決される。
  result.push(LayoutNode::Anchor(AnchorId::Heading(key)));
  if let Some(label) = label {
    result.push(LayoutNode::Anchor(AnchorId::Label(label)));
  }

  result.push(LayoutNode::VBox {
    children,
    margin_bottom: heading_style.bottom_margin,
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    alignment: heading_style.alignment,
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
    color::Color,
    document::TextAlignment,
    semantics::CitationId,
    style::{NumberTitleTemplate, Style as ReadStyle},
    typeset::{
      boxes::{AnchorId, LinkTarget},
      lowering::{
        HeadingRecord, lower_sources_with_headings,
        test_support::{analyzed, context, inline_text, inlines, lower, paragraph_layout},
      },
    },
  };

  /// 基底スタイルのプレーンなタイトルノード 1 個を作る
  fn plain_title(ctx: &LoweringContext<'_>, level: HeadingLevel, text: &str) -> Vec<InlineNode> {
    return vec![InlineNode::Text(text.to_string(), title_style(ctx, level))];
  }

  /// `nodes` から見出し `VBox` の段落内容を取り出す
  fn heading_children(nodes: &[LayoutNode]) -> &[InlineNode] {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(paragraph_layout(&children[0]).content.as_slice()),
        _ => return None,
      })
      .expect("VBox が出力されるはず");
  }

  #[test]
  fn heading_plain_text_formats_units() {
    let style = ReadStyle::default();
    let document = analyzed("\\section{速さ \\unit{m.s^{-1}}}");

    let (_, headings) = lower_sources_with_headings(&context(&style), &document);

    assert_eq!(headings[0].title_plain, "速さ m\u{202F}s\u{207B}\u{00B9}");
  }

  #[test]
  fn heading_plain_text_formats_quantities() {
    let style = ReadStyle::default();
    let document = analyzed("\\section{距離 \\qty{30}{\\degree} と \\qty{5}{km}}");

    let (_, headings) = lower_sources_with_headings(&context(&style), &document);

    assert_eq!(headings[0].title_plain, "距離 30\u{00B0} と 5\u{202F}km");
  }

  #[test]
  fn heading_plain_text_formats_numbers_by_style() {
    let style = ReadStyle::default();
    let document = analyzed("\\section{長さ \\num{-12345e3}}");

    let (_, headings) = lower_sources_with_headings(&context(&style), &document);

    assert_eq!(headings[0].title_plain, "長さ \u{2212}12\u{202F}345\u{205F}\u{00D7}\u{205F}10\u{00B3}");
  }

  #[test]
  fn formula_with_resolved_content_separates_heading_text() {
    // 解決後の中身がある数式は、組み立てると何も残らなくても（`{}`）1 つのノードで、前後のテキストを畳まない
    let nodes = lower(&ReadStyle::default(), &analyzed("\\section{AV${}$AV}\n"));

    let children = heading_children(&nodes);
    assert!(
      matches!(children, [InlineNode::Text(a, _), InlineNode::Math(_), InlineNode::Text(b, _)] if a.ends_with("AV") && b == "AV"),
      "{children:?}"
    );
  }

  #[test]
  fn lower_heading_uses_style_template() {
    let mut style = ReadStyle::default();
    style.heading.section.format = NumberTitleTemplate::parse("[{number}] {title}");
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Custom Title");

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "4.7", || return title.clone(), None, HeadingKey::new(0), None);

    let children = heading_children(&nodes);
    let text = match &children[0] {
      InlineNode::Text(text, _) => text.clone(),
      other => panic!("Text ノードが期待されます: {other:?}"),
    };
    assert_eq!(text, "[4.7] Custom Title");
  }

  #[test]
  fn lower_heading_preserves_styled_title() {
    let style = ReadStyle::default();

    let nodes = lower(&style, &analyzed("\\section{Intro \\italic{Italic}}\n"));

    let children = heading_children(&nodes);
    let heading_size = style.heading[HeadingLevel::Section].font_size;
    let italic = children
      .iter()
      .find_map(|n| match n {
        InlineNode::Text(t, s) if t == "Italic" => return Some(*s),
        _ => return None,
      })
      .expect("イタリック部分の Text があるはず");
    assert_eq!(italic.typeface, Typeface::SerifItalic);
    assert_eq!(italic.font_size, heading_size, "フォントサイズは見出しスタイルを継承する");
  }

  #[test]
  fn lower_heading_emits_anchor_with_label() {
    let style = ReadStyle::default();
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    let nodes = lower_heading(
      &ctx,
      HeadingLevel::Section,
      "1",
      || return title.clone(),
      Some(LabelId::new("sec:intro")),
      HeadingKey::new(3),
      None,
    );

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
    let style = ReadStyle::default();
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "1", || return title.clone(), None, HeadingKey::new(0), None);

    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    let keep_idx = nodes.iter().position(|n| matches!(n, LayoutNode::KeepWithNext)).expect("KeepWithNext が出るはず");
    assert!(keep_idx > vbox_idx, "KeepWithNext は VBox の後に出る: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "改ページは出ない: {nodes:?}");
  }

  #[test]
  fn lower_heading_with_page_break_after_omits_keep_with_next() {
    let mut style = ReadStyle::default();
    style.heading.section.page_break_after = true;
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "1", || return title.clone(), None, HeadingKey::new(0), None);

    assert!(nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "強制改ページが出るはず: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::KeepWithNext)), "KeepWithNext は出ない: {nodes:?}");
  }

  /// インライン列から脚注の (表示番号, 通し index) を文書順に集める
  fn footnotes<'a>(nodes: impl IntoIterator<Item = &'a InlineNode>) -> Vec<(u32, u32)> {
    return nodes
      .into_iter()
      .filter_map(|n| match n {
        InlineNode::Footnote { number, index, .. } => return Some((*number, *index)),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn heading_format_without_title_placeholder_does_not_consume_footnote_number() {
    let mut style = ReadStyle::default();
    style.heading.section.format = NumberTitleTemplate::parse("{number}");

    let nodes = lower(&style, &analyzed("\\section{Intro\\footnote{in title}}\n\nbody\\footnote{in body}\n"));

    assert_eq!(footnotes(inlines(&nodes)), vec![(1, 0)], "{nodes:?}");
  }

  #[test]
  fn heading_format_with_two_title_placeholders_lowers_title_twice() {
    let mut style = ReadStyle::default();
    style.heading.section.format = NumberTitleTemplate::parse("{title} / {title}");

    let nodes = lower(&style, &analyzed("\\section{Intro\\footnote{n}}\n"));

    assert_eq!(footnotes(heading_children(&nodes)), vec![(1, 0), (2, 1)], "{nodes:?}");
  }

  #[test]
  fn ref_in_heading_title_is_resolved_to_internal_link() {
    let style = ReadStyle::default();

    let nodes = lower(&style, &analyzed("\\chapter[label=ch:other]{Other}\n\n\\section{\\ref{ch:other}}\n"));

    let children = nodes
      .iter()
      .rev()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => return Some(paragraph_layout(&children[0]).content.as_slice()),
        _ => return None,
      })
      .expect("section の VBox があるはず");
    let link = children
      .iter()
      .find_map(|n| match n {
        InlineNode::Link { target, children } => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, LinkTarget::Internal(AnchorId::Label(LabelId::new("ch:other"))));
    assert!(matches!(&link.1[0], InlineNode::Text(t, _) if t == "Chapter 1"), "{:?}", link.1);
  }

  /// `nodes` から見出し `VBox` の揃えを取り出す
  #[expect(
    clippy::unwrap_in_result,
    reason = "VBox が出ないのは lowering の不具合で、テストを失敗させるため panic が必要"
  )]
  fn heading_alignment(nodes: &[LayoutNode]) -> Option<TextAlignment> {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { alignment, .. } => return Some(*alignment),
        _ => return None,
      })
      .expect("VBox が出力されるはず");
  }

  #[test]
  fn lower_heading_leaves_alignment_to_enclosing_list_by_default() {
    let style = ReadStyle::default();
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "1", || return title.clone(), None, HeadingKey::new(0), None);

    assert_eq!(heading_alignment(&nodes), None, "未指定なら外側の揃え（[text].alignment）に従う");
  }

  #[test]
  fn lower_heading_uses_level_alignment() {
    let mut style = ReadStyle::default();
    style.heading.section.alignment = Some(TextAlignment::Right);
    let ctx = context(&style);
    let title = plain_title(&ctx, HeadingLevel::Section, "Intro");

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "1", || return title.clone(), None, HeadingKey::new(0), None);

    assert_eq!(heading_alignment(&nodes), Some(TextAlignment::Right));
  }

  /// `.sei` ソースを lower した見出しの記録
  fn heading_records(style: &ReadStyle, source: &str) -> Vec<HeadingRecord> {
    let (_layout, headings) = lower_sources_with_headings(&context(style), &analyzed(source));
    return headings;
  }

  /// インライン列を、リンクの子と脚注の本体まで降りて文書順に並べる
  fn flatten(nodes: &[InlineNode]) -> Vec<&InlineNode> {
    let mut out = Vec::new();
    for node in nodes {
      out.push(node);
      match node {
        InlineNode::Link { children, .. } => out.extend(flatten(children)),
        InlineNode::Footnote { body, .. } => out.extend(flatten(body)),
        _ => {},
      }
    }
    return out;
  }

  /// 列の `Text` の（文字列, スタイル）を文書順に集める
  fn texts(nodes: &[InlineNode]) -> Vec<(&str, TextStyle)> {
    return flatten(nodes).into_iter().filter_map(inline_text).collect();
  }

  #[test]
  fn toc_label_keeps_inline_math() {
    let headings = heading_records(&ReadStyle::default(), "\\section{$E=mc^{2}$ の導出}\n");

    let number = format!("{} ", headings[0].number);
    assert!(
      matches!(headings[0].toc_label.as_slice(), [InlineNode::Text(n, _), InlineNode::Math(_), InlineNode::Text(rest, _)] if *n == number && rest == " の導出"),
      "{:?}",
      headings[0].toc_label
    );
    assert!(texts(&headings[0].toc_label).iter().all(|(t, _)| return !t.contains("[Math]")));
  }

  #[test]
  fn toc_label_layers_decoration_over_toc_style() {
    let mut style = ReadStyle::default();
    style.heading.section.typeface = Typeface::SansSerif;
    style.toc.font_size = Length::pt(11.0);

    let headings = heading_records(&style, "\\section{A \\bold{B} \\color[color=#ff0000]{C}}\n");

    let texts = texts(&headings[0].toc_label);
    let style_of = |needle: &str| {
      return texts.iter().find(|(t, _)| return t.contains(needle)).map(|(_, s)| return *s).expect("字があるはず");
    };
    assert_eq!(style_of("A"), toc_entry_style(&style), "外側は目次の行の書体（見出しの SansSerif を継がない）");
    assert_eq!(style_of("B").typeface, Typeface::SerifBold);
    assert_eq!(style_of("B").font_size, Length::pt(11.0));
    assert_eq!(style_of("C").color, Some(Color::new(0xff, 0x00, 0x00)));
    assert_eq!(style_of("C").typeface, Typeface::Serif);
  }

  #[test]
  fn toc_label_has_no_links() {
    let blue = Color::new(0x00, 0x00, 0xff);
    let mut style = ReadStyle::default();
    style.hyperref.link_color = Some(blue);
    style.hyperref.url_color = Some(blue);
    style.hyperref.cite_color = Some(blue);
    let analyzed = analyzed(
      "\\chapter[label=ch:a]{A}\n\n\\section{\\ref{ch:a} \\href{https://example.com}{リンク} \\cite{kwan2014}}\n",
    );
    let site = analyzed.citation_sites().next().expect("引用箇所が 1 件あるはず");
    let document = analyzed.with_generated_citations_for_test(
      vec![(
        site,
        vec![GeneratedInline::InternalLink {
          target: CitationId::new("kwan2014"),
          children: vec![GeneratedInline::Text("[1]".to_string())],
        }],
      )],
      None,
    );

    let (_layout, headings) = lower_sources_with_headings(&context(&style), &document);

    let label = &headings[1].toc_label;
    assert!(!flatten(label).iter().any(|n| matches!(n, InlineNode::Link { .. })), "{label:?}");
    let joined: String = texts(label).iter().map(|(t, _)| return *t).collect();
    for expected in ["Chapter 1", "リンク", "[1]"] {
      assert!(joined.contains(expected), "{expected} を含むはず: {joined}");
    }
    assert!(texts(label).iter().all(|(_, s)| return s.color.is_none()), "リンク色を付けない: {label:?}");
  }

  #[test]
  fn toc_label_does_not_consume_footnote_numbers() {
    let style = ReadStyle::default();
    let document = analyzed("\\section{A\\footnote{x}}\n\n本文\\footnote{y}\n");

    let (layout, headings) = lower_sources_with_headings(&context(&style), &document);

    assert!(
      !flatten(&headings[0].toc_label)
        .iter()
        .any(|n| matches!(n, InlineNode::Footnote { .. } | InlineNode::Raise { .. })),
      "{:?}",
      headings[0].toc_label
    );
    assert_eq!(footnotes(inlines(&layout)), vec![(2, 1)], "本文の脚注は見出しの脚注の次: {layout:?}");
  }

  #[test]
  fn toc_label_turns_line_break_into_space() {
    let headings = heading_records(&ReadStyle::default(), "\\section{A\\\\B}\n");

    let expected = format!("{} A B", headings[0].number);
    assert!(
      matches!(headings[0].toc_label.as_slice(), [InlineNode::Text(t, _)] if *t == expected),
      "{:?}",
      headings[0].toc_label
    );
  }

  #[test]
  fn toc_label_joins_like_the_plain_label() {
    let style = toc_entry_style(&ReadStyle::default());
    let title = || return vec![InlineNode::Text("T".to_string(), style)];
    let text_of = |nodes: Vec<InlineNode>| -> Vec<String> {
      return nodes.iter().filter_map(inline_text).map(|(t, _)| return t.to_string()).collect();
    };

    assert_eq!(text_of(toc_label("", title(), style)), vec!["T"]);
    assert_eq!(text_of(toc_label("1", Vec::new(), style)), vec!["1"]);
    assert_eq!(text_of(toc_label("1", title(), style)), vec!["1 T"]);
  }

  /// `\section{title}` のしおりの題目（番号を除く平文）
  fn bookmark(title: &str) -> String {
    return heading_records(&ReadStyle::default(), &format!("\\section{{{title}}}\n"))[0].title_plain.clone();
  }

  #[test]
  fn bookmark_math_keeps_chars_and_symbols() {
    assert_eq!(bookmark(r"$a+\alpha$"), "a+α");
    assert_eq!(bookmark("$a + b$"), "a+b");
    assert_eq!(bookmark("前 $x$ 後"), "前 x 後");
  }

  #[test]
  fn bookmark_math_explicit_spaces_collapse() {
    assert_eq!(bookmark(r"$a\thinspace b$"), "a b");
    assert_eq!(bookmark(r"$a\thinspace\quad b$"), "a b");
    assert_eq!(bookmark(r"$a\medspace b\thickspace c$"), "a b c");
    assert_eq!(bookmark(r"$a\negthinspace b$"), "ab");
  }

  #[test]
  fn bookmark_math_operator_name_spacing() {
    assert_eq!(bookmark(r"$\sin x$"), "sin x");
    assert_eq!(bookmark(r"$\sin^{2} x$"), "sin² x");
    assert_eq!(bookmark(r"$\sin\quad x$"), "sin x");
    assert_eq!(bookmark(r"$x \sin$"), "xsin");
  }

  #[test]
  fn bookmark_math_prose_and_group() {
    assert_eq!(bookmark(r"$x\text{ if }y$"), "x if y");
    assert_eq!(bookmark("${ab}$"), "ab");
  }

  #[test]
  fn bookmark_math_scripts() {
    assert_eq!(bookmark("$x^{2}$"), "x²");
    assert_eq!(bookmark("$x_{10}$"), "x₁₀");
    assert_eq!(bookmark("$x^{-1}$"), "x⁻¹");
    assert_eq!(bookmark("$x^{n}$"), "x^n");
    assert_eq!(bookmark("$x^{n+1}$"), "x^(n+1)");
    assert_eq!(bookmark("$x^{ab}$"), "x^ab");
    assert_eq!(bookmark("$x^{a2}$"), "x^a2");
    assert_eq!(bookmark("${a+b}^{2}$"), "(a+b)²");
    assert_eq!(bookmark("$x_{i}^{2}$"), "x_i²");
    assert_eq!(bookmark("$ab^{2}$"), "ab²");
    assert_eq!(bookmark(r"$\frac{a}{b}^{2}$"), "(a/b)²");
    assert_eq!(bookmark(r"$\paren{x}^{2}$"), "(x)²");
  }

  #[test]
  fn bookmark_math_parenthesized_text_operand_is_simple() {
    assert_eq!(bookmark("$x^{(n)}$"), "x^(n)");
    assert_eq!(bookmark("$x^{(a)+(b)}$"), "x^((a)+(b))");
  }

  #[test]
  fn bookmark_math_multi_char_base_is_wrapped() {
    assert_eq!(bookmark("${ab}^{2}$"), "(ab)²");
    assert_eq!(bookmark("${xy}_{i}$"), "(xy)_i");
    assert_eq!(bookmark("$ab^{2}$"), "ab²");
    assert_eq!(bookmark(r"$\sin^{2} x$"), "sin² x");
  }

  #[test]
  fn bookmark_math_root_base_is_wrapped() {
    assert_eq!(bookmark(r"$\sqrt{x}^{2}$"), "(√x)²");
    assert_eq!(bookmark(r"$\sqrt[3]{x}^{2}$"), "(∛x)²");
  }

  #[test]
  fn bookmark_math_bare_operand_is_ended_by_a_space() {
    assert_eq!(bookmark("$x^{n}y$"), "x^n y");
    assert_eq!(bookmark("$a_{n}b_{n}$"), "a_n b_n");
    assert_eq!(bookmark(r"$\frac{1}{2}mv^{2}$"), "1/2 mv²");
    assert_eq!(bookmark(r"$\frac{a}{b}c$"), "a/b c");
    assert_eq!(bookmark(r"$\sqrt{2}x$"), "√2 x");
    assert_eq!(bookmark(r"$\overline{x}y$"), "¯x y");
    assert_eq!(bookmark(r"$x^{n}\alpha$"), "x^n α");
    assert_eq!(bookmark("${x^{n}}y$"), "x^n y");
    // 閉じた被演算子・演算子・スクリプトの前には入れない
    assert_eq!(bookmark("$x^{2}y$"), "x²y");
    assert_eq!(bookmark("$x^{n+1}y$"), "x^(n+1)y");
    assert_eq!(bookmark("$x^{n}+y$"), "x^n+y");
    assert_eq!(bookmark("$x^{n}_{2}$"), "x^n₂");
  }

  #[test]
  fn bookmark_math_nested_script_wraps_outer_operand() {
    assert_eq!(bookmark("$x^{2^{n}}$"), "x^(2^n)");
  }

  #[test]
  fn bookmark_math_minus_symbol_is_a_superscript_minus() {
    // U+2212 は `\unit` のしおりの指数と同じく上付きの負号にする
    assert_eq!(bookmark(r"$x^{\minus 1}$"), "x⁻¹");
  }

  #[test]
  fn bookmark_math_empty_parts_vanish() {
    assert_eq!(bookmark("x${}$y"), "xy");
    assert_eq!(bookmark("$x^{}$"), "x");
  }

  #[test]
  fn bookmark_math_fractions_and_binomials() {
    assert_eq!(bookmark(r"$\frac{a}{b}$"), "a/b");
    assert_eq!(bookmark(r"$\frac{a+b}{2}$"), "(a+b)/2");
    assert_eq!(bookmark(r"$\binom{n}{k}$"), "(n¦k)");
  }

  #[test]
  fn bookmark_math_roots() {
    assert_eq!(bookmark(r"$\sqrt{x}$"), "√x");
    assert_eq!(bookmark(r"$\sqrt{a+b}$"), "√(a+b)");
    assert_eq!(bookmark(r"$\sqrt[3]{x}$"), "∛x");
    assert_eq!(bookmark(r"$\sqrt[4]{x}$"), "∜x");
    assert_eq!(bookmark(r"$\sqrt[n]{x}$"), "√(n&x)");
  }

  #[test]
  fn bookmark_math_accents_and_bars() {
    assert_eq!(bookmark(r"$\hat{x}$"), "x\u{0302}");
    assert_eq!(bookmark(r"$\widehat{xy}$"), "(xy)\u{0302}");
    assert_eq!(bookmark(r"$\overline{x}$"), "¯x");
    assert_eq!(bookmark(r"$\underline{a+b}$"), "▁(a+b)");
  }

  #[test]
  fn bookmark_math_fences() {
    assert_eq!(bookmark(r"$\paren{x}$"), "(x)");
    assert_eq!(bookmark(r"$\abs{x}$"), "|x|");
    assert_eq!(bookmark(r"$\paren{a \given b}$"), "(a|b)");
  }

  #[test]
  fn bookmark_math_arrays() {
    assert_eq!(bookmark(r"$\begin{matrix}[delimiter=paren]a & b \\ c & d\end{matrix}$"), "(■(a&b@c&d))");
    assert_eq!(bookmark(r"$\begin{matrix}a & b\end{matrix}$"), "■(a&b)");
    assert_eq!(
      bookmark(r"$\begin{cases}x & \text{if } x \geq 0 \\ -x & \text{otherwise}\end{cases}$"),
      "Ⓒ(x&if x≥0@-x&otherwise)"
    );
  }

  #[test]
  fn bookmark_math_alphanumerics_become_plain() {
    assert_eq!(bookmark(r"$\mathbold{x}+\mathdoublestruck{R}$"), "x+R");
    assert_eq!(bookmark("$\u{1D431}+\u{1D466}$"), "x+y");
  }

  #[test]
  fn bookmark_math_styled_is_transparent_to_script_base() {
    assert_eq!(bookmark(r"$\mathbold{a+b}^{2}$"), "a+b²");
  }

  #[test]
  fn bookmark_line_break_becomes_space() {
    assert_eq!(bookmark(r"前半\\後半"), "前半 後半");
  }
}
