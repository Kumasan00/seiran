//! 段落（`DocNode::Paragraph`）の lowering
//!
//! 段落内のインライン要素を展開してフラットな `LayoutNode` 列に変換します。
//! 段落間のアキは `Vkern`（`text.paragraph_spacing`）で構造的に表します。
//! 段落先頭行の字下げは先頭に水平カーン（`ctx.first_line_indent`）を前置して表します。

use model::InlineNode;

use super::{
  LoweringContext, LoweringError,
  counter::CounterRegistry,
  inline::lower_inline,
  layout_node::{LayoutNode, TextStyle},
};

/// 段落をレイアウトノードに変換する
///
/// `ctx.first_line_indent` が正のとき、結果先頭に水平カーンを前置する。貪欲法の行分割は
/// 行頭の `Kern` を「先頭行だけに効く幅」として消費するため、折り返し 2 行目以降・`\\` 直後の
/// 行には字下げが効かず、「段落先頭のみ字下げ」が自動的に成立する。
/// `registry` は段落内の `\footnote` 採番（`CounterRegistry::next_footnote_index`）に使う。
///
/// ## TODO
///
/// - [ ] 段落内テキストの結合最適化（隣接する同スタイルの `Text` をまとめる）
pub(super) fn lower_paragraph(
  ctx: &LoweringContext,
  inlines: &[InlineNode],
  registry: &mut CounterRegistry,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let default_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.body_font_kind,
    color: None,
  };

  let mut result = Vec::new();

  // `\noindent`（[`InlineNode::NoIndent`] マーカー）が段落にあれば字下げを抑止する。位置検証は
  // パーサ（`evaluate_children`）が段落先頭に限定済みなので、ここでは存在の有無だけを見る。
  let suppress_indent = inlines.iter().any(|inline| matches!(inline, InlineNode::NoIndent));

  // 段落先頭行の字下げ。先頭に水平カーンを置くと、貪欲法ブレーカが先頭行だけ右へずらして
  // 折り返し幅を狭める（2 行目以降には残らない）。0pt のとき・`\noindent` 指定時は何も足さない。
  if ctx.first_line_indent.to_pt() > 0.0 && !suppress_indent {
    result.push(LayoutNode::Kern {
      length: ctx.first_line_indent,
    });
  }

  for inline in inlines {
    // 非描画マーカーは出力に寄与しないので読み飛ばす
    if matches!(inline, InlineNode::NoIndent) {
      continue;
    }
    result.extend(lower_inline(ctx, inline, default_style, registry)?);
  }

  // 段落間スペースは縦カーンで構造的に表す（段落の行送り自体は縦組版層が担う）
  result.push(LayoutNode::Vkern {
    length: ctx.style.text.paragraph_spacing,
  });

  return Ok(result);
}

#[cfg(test)]
mod tests {
  use config::Style as ReadStyle;

  use super::*;

  #[test]
  fn paragraph_appends_single_trailing_vkern_with_paragraph_spacing() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [InlineNode::Text("hello".to_string())];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran())
      .expect("解決済みテキストなので失敗しない");

    // Assert — 末尾は paragraph_spacing 値の Vkern が 1 つだけ
    let LayoutNode::Vkern { length } = nodes.last().expect("末尾要素") else {
      panic!("末尾は Vkern であるべき: {nodes:?}");
    };
    assert!((length.to_pt() - style.text.paragraph_spacing.to_pt()).abs() < f32::EPSILON);
    let vkern_count = nodes.iter().filter(|n| matches!(n, LayoutNode::Vkern { .. })).count();
    assert_eq!(vkern_count, 1, "段落末の Vkern は 1 つだけ: {nodes:?}");
  }

  #[test]
  fn paragraph_text_uses_default_style_from_core_text() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [InlineNode::Text("body".to_string())];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — 本文 Text は text.font_kind / default_font_size を持つ
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("先頭は Text であるべき: {nodes:?}");
    };
    assert_eq!(text, "body");
    assert_eq!(text_style.font_kind, style.text.font_kind);
    assert_eq!(text_style.font_size, ctx.default_font_size());
  }

  #[test]
  fn paragraph_prepends_first_line_indent_kern_when_positive() {
    // Arrange — first_line_indent を 15pt にした文脈
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_first_line_indent(model::Length::pt(15.0));
    let inlines = [InlineNode::Text("body".to_string())];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — 先頭は first_line_indent 量の Kern、その次が本文 Text
    let LayoutNode::Kern { length } = &nodes[0] else {
      panic!("先頭は字下げ Kern であるべき: {nodes:?}");
    };
    assert!((length.to_pt() - 15.0).abs() < f32::EPSILON);
    assert!(matches!(&nodes[1], LayoutNode::Text(t, _) if t == "body"));
  }

  #[test]
  fn paragraph_noindent_marker_suppresses_indent_kern() {
    // Arrange — first_line_indent を 15pt にした文脈で、先頭に NoIndent マーカーがある段落
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_first_line_indent(model::Length::pt(15.0));
    let inlines = [InlineNode::NoIndent, InlineNode::Text("body".to_string())];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — 字下げ Kern は出ず、マーカーも描画されず、先頭は本文 Text
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::Kern { .. })), "字下げ Kern は抑止される: {nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "body"), "先頭は本文 Text: {nodes:?}");
  }

  #[test]
  fn paragraph_omits_first_line_indent_kern_by_default() {
    // Arrange — 既定（first_line_indent = 0pt）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [InlineNode::Text("body".to_string())];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — 先頭は本文 Text（字下げ Kern は無い）
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "body"), "先頭は本文 Text: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::Kern { .. })), "字下げ Kern は出ない: {nodes:?}");
  }

  #[test]
  fn paragraph_preserves_inline_order() {
    // Arrange — 3 つのテキストインラインを与える
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [
      InlineNode::Text("one".to_string()),
      InlineNode::Text("two".to_string()),
      InlineNode::Text("three".to_string()),
    ];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — Text ノードが入力順に並ぶ
    let texts: Vec<&str> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => return Some(t.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(texts, vec!["one", "two", "three"]);
  }

  #[test]
  fn paragraph_ref_becomes_placeholder() {
    // Arrange — \ref は即時解決せず LayoutNode::Ref プレースホルダになる
    // （解決・未解決エラーは pass2 = resolve::resolve_refs が担う。lower_paragraph 単体では実行しない）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [InlineNode::Ref {
      label: "eq:missing".to_string(),
      span: model::Span::DUMMY,
    }];

    // Act
    let nodes =
      lower_paragraph(&ctx, &inlines, &mut CounterRegistry::default_for_seiran()).expect("即時エラーにはならない");

    // Assert
    assert!(
      nodes.iter().any(|n| matches!(n, LayoutNode::Ref { label, .. } if label == "eq:missing")),
      "Ref プレースホルダが残るはず: {nodes:?}"
    );
  }
}
