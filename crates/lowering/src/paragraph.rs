//! 段落（`DocNode::Paragraph`）の lowering
//!
//! 段落内のインライン要素を展開してフラットな `LayoutNode` 列に変換します。
//! 段落間のアキは `Vkern`（`text.paragraph_spacing`）で構造的に表します。

use document::InlineNode;

use super::{LoweringContext, LoweringError, inline::lower_inline};
use crate::layout_node::{LayoutNode, TextStyle};

/// 段落をレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] 段落先頭のインデント（字下げ）を追加する
/// - [ ] 段落内テキストの結合最適化（隣接する同スタイルの `Text` をまとめる）
pub(super) fn lower_paragraph(ctx: &LoweringContext, inlines: &[InlineNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let default_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.style.text.font_kind,
    color: None,
  };

  let mut result = Vec::new();

  for inline in inlines {
    result.extend(lower_inline(ctx, inline, default_style)?);
  }

  // 段落間スペースは縦カーンで構造的に表す（段落の行送り自体は縦組版層が担う）
  result.push(LayoutNode::Vkern {
    length: ctx.style.text.paragraph_spacing,
  });

  return Ok(result);
}

#[cfg(test)]
mod tests {
  use read_style::Style as ReadStyle;

  use super::*;

  #[test]
  fn paragraph_appends_single_trailing_vkern_with_paragraph_spacing() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [InlineNode::Text("hello".to_string())];

    // Act
    let nodes = lower_paragraph(&ctx, &inlines).expect("解決済みテキストなので失敗しない");

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
    let nodes = lower_paragraph(&ctx, &inlines).expect("失敗しない");

    // Assert — 本文 Text は text.font_kind / default_font_size を持つ
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("先頭は Text であるべき: {nodes:?}");
    };
    assert_eq!(text, "body");
    assert_eq!(text_style.font_kind, style.text.font_kind);
    assert!((text_style.font_size - ctx.default_font_size()).abs() < f32::EPSILON);
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
    let nodes = lower_paragraph(&ctx, &inlines).expect("失敗しない");

    // Assert — Text ノードが入力順に並ぶ
    let texts: Vec<&str> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => Some(t.as_str()),
        _ => None,
      })
      .collect();
    assert_eq!(texts, vec!["one", "two", "three"]);
  }

  #[test]
  fn paragraph_propagates_unresolved_ref_error() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let inlines = [InlineNode::Ref {
      label: "eq:missing".to_string(),
      number: None,
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }];

    // Act
    let err = lower_paragraph(&ctx, &inlines).expect_err("未解決 Ref はエラー");

    // Assert
    let LoweringError::UnresolvedReference { label, .. } = err else {
      panic!("UnresolvedReference が期待されます: {err:?}");
    };
    assert_eq!(label, "eq:missing");
  }
}
