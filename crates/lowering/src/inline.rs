//! インライン要素（`InlineNode`）の lowering
//!
//! 親から継承されたスタイル（`parent_style`）を基に、インライン要素の種類に応じて
//! フォント種別やサイズを変更します。

use parser::document::InlineNode;
use types::FontKind;

use super::{LoweringContext, LoweringError, math::lower_inline_math};
use crate::layout_node::{LayoutNode, TextStyle};

/// インライン要素をレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] Emphasis / Strong のネスト対応（イタリック内の強調 → ボールドイタリック等）
/// - [ ] スタイルスタック方式に変更して、任意深さのネストに対応する
pub(super) fn lower_inline(
  ctx: &LoweringContext,
  inline: &InlineNode,
  parent_style: TextStyle,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match inline {
    InlineNode::Text(text) => {
      return Ok(vec![LayoutNode::Text(text.clone(), parent_style)]);
    },
    InlineNode::Emphasis(children) => {
      // TODO: ネスト対応（イタリック内の強調は通常体に戻す等）
      let italic_style = TextStyle {
        font_size: parent_style.font_size,
        font_kind: FontKind::SerifItalic,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, italic_style)?);
      }
      return Ok(result);
    },
    InlineNode::Strong(children) => {
      let bold_style = TextStyle {
        font_size: parent_style.font_size,
        font_kind: FontKind::SerifBold,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, bold_style)?);
      }
      return Ok(result);
    },
    InlineNode::Code(children) => {
      let mono_style = TextStyle {
        font_size: parent_style.font_size,
        font_kind: FontKind::Monospace,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, mono_style)?);
      }
      return Ok(result);
    },
    InlineNode::SansSerif(children) => {
      let sans_style = TextStyle {
        font_size: parent_style.font_size,
        font_kind: FontKind::SansSerif,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, sans_style)?);
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
    InlineNode::Ref { label, number } => {
      // 評価器の pass2 で参照解決が済んでいれば number は Some。未解決のまま
      // lowering に到達した場合は `LoweringError::UnresolvedReference` で報告する。
      let Some(resolved) = number.clone() else {
        return Err(LoweringError::UnresolvedReference {
          label: label.clone(),
        });
      };
      return Ok(vec![LayoutNode::Text(resolved, parent_style)]);
    },
  }
}

/// インライン要素をプレーンテキストに変換する（一時的なヘルパー）
///
/// ## TODO
///
/// - [ ] 移行完了後にこの関数は不要になる（インライン要素は `lower_inline` で
///   個別にスタイル付きテキストに変換されるため）
/// - [ ] 見出しタイトルのインライン要素に対応するまでの間の暫定実装
pub(super) fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> Result<String, LoweringError> {
  let mut text = String::new();
  for inline in inlines {
    match inline {
      InlineNode::Text(s) => text.push_str(s),
      InlineNode::Emphasis(children)
      | InlineNode::Strong(children)
      | InlineNode::Code(children)
      | InlineNode::SansSerif(children) => {
        text.push_str(&inline_nodes_to_plain_text(children)?);
      },
      InlineNode::InlineMath(_) => text.push_str("[Math]"),
      InlineNode::Symbol(ch) => text.push(*ch),
      InlineNode::LineBreak => text.push('\n'),
      InlineNode::Ref { label, number } => {
        let Some(s) = number else {
          return Err(LoweringError::UnresolvedReference {
            label: label.clone(),
          });
        };
        text.push_str(s);
      },
    }
  }
  return Ok(text);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_inline_text_to_plain() {
    // Arrange
    let inlines = vec![
      InlineNode::Text("Hello ".to_string()),
      InlineNode::Strong(vec![InlineNode::Text("world".to_string())]),
    ];

    // Act
    let result = inline_nodes_to_plain_text(&inlines).expect("解決済みノードのみなので失敗しないはず");

    // Assert
    assert_eq!(result, "Hello world");
  }
}
