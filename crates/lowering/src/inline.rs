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
        // 色は書体と直交するので、親から引き継いだ色を保持する
        color: parent_style.color,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, styled)?);
      }
      return Ok(result);
    },
    InlineNode::Colored { color, children } => {
      let colored = TextStyle {
        // フォントサイズ・書体は親から継承し、色だけを上書きする（書体と直交合成）
        font_size: parent_style.font_size,
        font_kind: parent_style.font_kind,
        color: Some(*color),
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, colored)?);
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
    InlineNode::Cite { keys, label, span } => {
      // CSL 整形ステージ（`citation::process_citations`）が lowering の前段で `label` を
      // 確定済みのはず。未確定のまま到達した場合は `Ref` と同様にエラーとして報告する。
      let Some(inlines) = label else {
        return Err(LoweringError::UnresolvedCitation {
          keys: keys.join(", "),
          span: *span,
        });
      };
      let mut result = Vec::new();
      for child in inlines {
        result.extend(lower_inline(ctx, child, parent_style)?);
      }
      return Ok(result);
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
      color: None,
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
  fn lower_inline_colored_overrides_color_keeps_font() {
    // Arrange — \color は親の font_kind / font_size を継承し、色だけ上書きする
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Colored {
      color: types::Color::new(0xff, 0x00, 0x00),
      children: vec![InlineNode::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: 10.0,
      font_kind: types::FontKind::SansSerif,
      color: None,
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, parent).expect("Text のみなので失敗しないはず");

    // Assert — font_kind は親 SansSerif を維持し、色だけが Some に上書きされる
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, types::FontKind::SansSerif);
    assert_eq!(text_style.color, Some(types::Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn lower_bold_inside_color_keeps_color() {
    // Arrange — \color[...]{\bold{x}} は内側で書体を変えても色が保持される（直交合成）
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Colored {
      color: types::Color::new(0x00, 0x80, 0x00),
      children: vec![InlineNode::Styled {
        kind: types::FontKind::SerifBold,
        children: vec![InlineNode::Text("x".to_string())],
      }],
    };
    let parent = TextStyle::new(10.0);

    // Act
    let nodes = lower_inline(&ctx, &inline, parent).expect("Text のみなので失敗しないはず");

    // Assert — 内側 \bold で SerifBold になっても色は外側 \color のまま
    let LayoutNode::Text(_, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text_style.font_kind, types::FontKind::SerifBold);
    assert_eq!(text_style.color, Some(types::Color::new(0x00, 0x80, 0x00)));
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
