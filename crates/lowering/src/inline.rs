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
      return Ok(lower_inline_math(math_nodes, parent_style.font_size, &ctx.style.math.script));
    },
    InlineNode::Symbol(ch) => {
      return Ok(vec![LayoutNode::Text(ch.to_string(), parent_style)]);
    },
    InlineNode::LineBreak => {
      return Ok(vec![LayoutNode::LineBreak]);
    },
    InlineNode::NoIndent => {
      // 非描画の字下げ抑止マーカー。段落字下げの抑止は `lower_paragraph` が担い、
      // 通常は同関数がマーカーを除去するためここには到達しないが、防御的に空を返す。
      return Ok(Vec::new());
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
      // 表示色はハイパーリンクの `link_color`（既定青、明示 `\color` があればそちらを優先）。
      let style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::Internal(label.clone()),
        children: vec![LayoutNode::Text(resolved, style)],
      }]);
    },
    InlineNode::Link { url, children } => {
      // 外部リンク（`\url` / `\href`）。表示テキストを External リンクで囲む。
      // 表示色はハイパーリンクの `url_color`（既定青、明示 `\color` があればそちらを優先）。
      let style = with_link_color(parent_style, ctx.style.hyperref.url_color);
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, style)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: inner,
      }]);
    },
    InlineNode::InternalLink { target, children } => {
      // 整形済みの内部リンク（`\cite` の各番号など）。色は親文脈（`Cite` 側で適用した
      // cite_color 等）から継承するため、ここでは色を触らずリンク領域で囲むだけにする。
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, parent_style)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::Internal(target.clone()),
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
      // 引用ラベル全体（括弧含む）に `cite_color` を適用する。番号部分は `InlineNode::InternalLink`
      // としてこの色を継承したまま内部リンクで囲まれる（明示 `\color` があればそちらを優先）。
      let style = with_link_color(parent_style, ctx.style.hyperref.cite_color);
      let mut result = Vec::new();
      for child in inlines {
        result.extend(lower_inline(ctx, child, style)?);
      }
      return Ok(result);
    },
  }
}

/// リンク表示テキストにハイパーリンク色を適用したスタイルを返す。
///
/// 明示的な `\color` を優先するため、親が既に色を持つ場合はそれを保ち、親が無色（`None`）の
/// ときだけ `link_color` をデフォルトとして適用する。`link_color` 自体が `None`（色指定なし）なら
/// 本文色（黒）を継承する（受け入れ条件: 色未指定時は黒継承）。
fn with_link_color(parent_style: TextStyle, link_color: Option<types::Color>) -> TextStyle {
  return TextStyle {
    color: parent_style.color.or(link_color),
    ..parent_style
  };
}

#[cfg(test)]
mod tests {
  use types::Length;

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
      font_size: Length::pt(10.0),
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
    assert_eq!(text_style.font_size, Length::pt(10.0));
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
      font_size: Length::pt(10.0),
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
    let parent = TextStyle::new(Length::pt(10.0));

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
    let parent = TextStyle::new(Length::pt(10.0));

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
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent).expect("失敗しない");

    // Assert — Link { External(url), [Text("ここ")] }
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::External("https://example.com".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "ここ"));
  }

  #[test]
  fn lower_ref_applies_link_color() {
    // Arrange — style で link_color を指定すると \ref の番号テキストに乗る
    let blue = types::Color::new(0x00, 0x00, 0xff);
    let mut style = read_style::Style::default();
    style.hyperref.link_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "sec:intro".to_string(),
      number: Some("1.2".to_string()),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0))).expect("解決済み Ref は失敗しない");

    // Assert — リンク子の Text が link_color を持つ
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_external_link_applies_url_color() {
    // Arrange — style で url_color を指定すると外部リンクの表示テキストに乗る
    let blue = types::Color::new(0x00, 0x00, 0xff);
    let mut style = read_style::Style::default();
    style.hyperref.url_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Link {
      url: "https://example.com".to_string(),
      children: vec![InlineNode::Text("ここ".to_string())],
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0))).expect("失敗しない");

    // Assert
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_ref_inherits_black_when_link_color_none() {
    // Arrange — link_color = None のとき、本文色（黒 = None）を継承する
    let mut style = read_style::Style::default();
    style.hyperref.link_color = None;
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "a".to_string(),
      number: Some("1".to_string()),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0))).expect("解決済み Ref は失敗しない");

    // Assert — 色は None（黒継承）
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, None);
  }

  #[test]
  fn lower_explicit_color_overrides_link_color() {
    // Arrange — \color[red]{\ref{a}} は明示色（赤）がリンク色より優先される
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let red = types::Color::new(0xff, 0x00, 0x00);
    let inline = InlineNode::Colored {
      color: red,
      children: vec![InlineNode::Ref {
        label: "a".to_string(),
        number: Some("1".to_string()),
        span: miette::SourceSpan::from((0_usize, 0_usize)),
      }],
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0))).expect("解決済み Ref は失敗しない");

    // Assert — 番号は赤（parent_style.color が Some なのでそちらを優先）
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(red));
  }

  #[test]
  fn lower_internal_link_maps_to_internal_target() {
    // Arrange — InternalLink は内部リンク（Internal(target)）に変換され、色は触らない
    let style = read_style::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::InternalLink {
      target: "cite:foo".to_string(),
      children: vec![InlineNode::Text("1".to_string())],
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0))).expect("失敗しない");

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal("cite:foo".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "1"));
  }

  #[test]
  fn lower_cite_label_applies_cite_color_and_links() {
    // Arrange — Cite ラベル内の InternalLink 番号は cite_color を継承しつつ内部リンクになる
    let blue = types::Color::new(0x00, 0x00, 0xff);
    let mut style = read_style::Style::default();
    style.hyperref.cite_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Cite {
      keys: vec!["foo".to_string()],
      label: Some(vec![InlineNode::InternalLink {
        target: "cite:foo".to_string(),
        children: vec![InlineNode::Text("1".to_string())],
      }]),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0))).expect("解決済み Cite は失敗しない");

    // Assert — Internal リンクで、番号テキストが cite_color を継承
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal("cite:foo".to_string()));
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }
}
