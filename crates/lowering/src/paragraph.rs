//! 段落（`DocNode::Paragraph`）の lowering
//!
//! 段落内のインライン要素を展開し、フラットな `LayoutNode::Text` のリストとして
//! レイアウトノードに変換します。段落間にはデフォルトのスペースを挿入します。

use parser::document::InlineNode;

use super::{LoweringContext, LoweringError, inline::lower_inline};
use crate::layout_node::{LayoutNode, TextStyle};

/// 段落をレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] インライン要素のスタイル変更（Emphasis → Italic 等）を反映する
/// - [ ] 段落先頭のインデント（字下げ）を追加する
/// - [ ] 段落内テキストの結合最適化（evaluator.rs の `merge_text` に相当するロジック）
pub(super) fn lower_paragraph(ctx: &LoweringContext, inlines: &[InlineNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let default_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: ctx.style.core.text.font_kind,
  };

  let mut result = Vec::new();

  for inline in inlines {
    result.extend(lower_inline(ctx, inline, default_style)?);
  }

  // 段落末に改行 + カーンを追加（段落間スペース）
  result.push(LayoutNode::LineBreak);
  result.push(LayoutNode::Kern {
    length: ctx.style.core.text.paragraph_spacing,
  });

  return Ok(result);
}
