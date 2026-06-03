//! 段落（`DocNode::Paragraph`）の lowering
//!
//! 段落内のインライン要素を展開し、フラットな `LayoutNode::Text` のリストとして
//! レイアウトノードに変換します。段落間にはデフォルトのスペースを挿入します。

use parser::document::InlineNode;
use types::FontKind;

use super::{LoweringContext, LoweringError, inline::lower_inline};
use crate::layout_node::{LayoutNode, Style};

/// 段落をレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] インライン要素のスタイル変更（Emphasis → Italic 等）を反映する
/// - [ ] 段落先頭のインデント（字下げ）を追加する
/// - [ ] 段落間スペースをスタイル設定で定義可能にする
/// - [ ] 段落内テキストの結合最適化（evaluator.rs の `merge_text` に相当するロジック）
pub(super) fn lower_paragraph(ctx: &LoweringContext, inlines: &[InlineNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let default_style = Style {
    font_size: ctx.default_font_size(),
    font_kind: FontKind::Serif,
  };

  let mut result = Vec::new();

  for inline in inlines {
    result.extend(lower_inline(ctx, inline, default_style)?);
  }

  // 段落末に改行 + カーンを追加（段落間スペース）
  // TODO: 段落間スペースをスタイル設定で制御する
  result.push(LayoutNode::LineBreak);
  result.push(LayoutNode::Kern {
    point: ctx.default_font_size(),
  });

  return Ok(result);
}
