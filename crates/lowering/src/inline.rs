//! インライン要素（`InlineNode`）の lowering
//!
//! 親から継承されたスタイル（`parent_style`）を基に、インライン要素の種類に応じて
//! フォント種別やサイズを変更します。

use document::InlineNode;

use super::{LoweringContext, LoweringError, math::lower_inline_math};
use crate::layout_node::{LayoutNode, TextStyle};

/// インライン要素をレイアウトノードに変換する
///
/// 書体指定（`InlineNode::Styled`）はパーサ段で `FontKind` まで確定しているため、
/// ここでは `font_kind` を完全に上書きするだけでよい（親スタイルとの合成はしない）。
pub(super) fn lower_inline(
  ctx: &LoweringContext,
  inline: &InlineNode,
  parent_style: TextStyle,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match inline {
    InlineNode::Text(text) => {
      return Ok(vec![LayoutNode::Text(text.clone(), parent_style)]);
    },
    InlineNode::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, styled)?);
      }
      return Ok(result);
    },
    InlineNode::InlineMath(math_nodes) => {
      return Ok(lower_inline_math(math_nodes, parent_style.font_size, &ctx.style.core.math));
    },
    InlineNode::Symbol(ch) => {
      return Ok(vec![LayoutNode::Text(ch.to_string(), parent_style)]);
    },
    InlineNode::LineBreak => {
      return Ok(vec![LayoutNode::LineBreak]);
    },
    InlineNode::Ref {
      label,
      number,
      span,
    } => {
      // 評価器の pass2 で参照解決が済んでいれば number は Some。未解決のまま
      // lowering に到達した場合は `LoweringError::UnresolvedReference` で報告する。
      let Some(resolved) = number.clone() else {
        return Err(LoweringError::UnresolvedReference {
          label: label.clone(),
          span: *span,
        });
      };
      return Ok(vec![LayoutNode::Text(resolved, parent_style)]);
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lower_inline_styled_overrides_parent_kind() {
    // Arrange — 太字文脈（親 SerifBold）の中の \italic は内側の SerifItalic に完全上書きされる
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Styled {
      kind: types::FontKind::SerifItalic,
      children: vec![InlineNode::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: 10.0,
      font_kind: types::FontKind::SerifBold,
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, parent).expect("Text のみなので失敗しないはず");

    // Assert — フォントサイズは親から継承し、font_kind は内側の指定になる
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, types::FontKind::SerifItalic);
    assert!((text_style.font_size - 10.0).abs() < f32::EPSILON);
  }
}
