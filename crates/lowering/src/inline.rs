//! インライン要素（`InlineNode`）の lowering
//!
//! 親から継承されたスタイル（`parent_style`）を基に、インライン要素の種類に応じて
//! フォント種別やサイズを変更します。

use document::InlineNode;
use types::LinkTarget;

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
      // 番号テキストを内部リンク（機構 B）で囲み、参照先アンカーへジャンプできるようにする。
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::Internal(label.clone()),
        children: vec![LayoutNode::Text(resolved, parent_style)],
      }]);
    },
    InlineNode::Link { url, children } => {
      // 外部リンク（`\url` / `\href`）。表示テキストを External リンクで囲む。
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, parent_style)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: inner,
      }]);
    },
    InlineNode::Cite { keys, label, .. } => {
      // CSL 整形ステージが `label` を確定済みならそれを描画する。未確定（CSL 整形未実装）
      // の暫定動作として、キー列をそのまま描画してパイプラインを通す。
      if let Some(inlines) = label {
        let mut result = Vec::new();
        for child in inlines {
          result.extend(lower_inline(ctx, child, parent_style)?);
        }
        return Ok(result);
      }
      return Ok(vec![LayoutNode::Text(keys.join(", "), parent_style)]);
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

  #[test]
  fn lower_resolved_ref_wraps_number_in_internal_link() {
    // Arrange — 解決済み Ref は番号テキストを内部リンク（Internal(label)）で囲む
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "sec:intro".to_string(),
      number: Some("1.2".to_string()),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    };
    let parent = TextStyle::new(10.0);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent).expect("解決済み Ref は失敗しない");

    // Assert — Link { Internal("sec:intro"), [Text("1.2")] }
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal("sec:intro".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "1.2"));
  }

  #[test]
  fn lower_external_link_maps_to_external_target() {
    // Arrange — InlineNode::Link は External リンクに変換され、表示テキストを子に持つ
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Link {
      url: "https://example.com".to_string(),
      children: vec![InlineNode::Text("ここ".to_string())],
    };
    let parent = TextStyle::new(10.0);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent).expect("失敗しない");

    // Assert — Link { External(url), [Text("ここ")] }
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::External("https://example.com".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "ここ"));
  }
}
