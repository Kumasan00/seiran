//! 引用ブロック（`DocNode::Quote`）の lowering
//!
//! 引用環境を「上マージン → 左右字下げ `VBox`（本体） → 下マージン」に変換する。本体は
//! [`config::read_style::QuoteStyle`] の `font_kind` を既定書体にして lower し、`quotation`（`kind` が
//! 段落先頭字下げありのとき）はブロック内段落へ `first_line_indent` を波及させる（`quote` は 0）。
//! 左右の字下げは `VBox` の `indent` / `right_indent` で表し、`crate::block::build_blocks` が配下の
//! 段落へ確定値を刻む。

use model::{Align, DocNode, Length, QuoteKind};

use super::{
  LoweringContext, LoweringError, PendingHeading, counter::CounterRegistry, layout_node::LayoutNode, lower_nodes_inner,
};

/// 引用ブロックをレイアウトノードに変換する
///
/// 構成: `Vkern(top_margin)` → 本体 `VBox`（左右 `indent` 字下げ）→ `Vkern(bottom_margin)`。
/// 本体は `font_kind` を既定書体にし、`quotation` のときだけ段落先頭字下げ（`first_line_indent`）を
/// 与える。`quote` は段落先頭字下げなし。
///
/// # Errors
///
/// 本体の lowering が返す [`LoweringError`]（未解決 `\ref` 等）を伝播する。
pub(super) fn lower_quote(
  ctx: &LoweringContext,
  kind: QuoteKind,
  body: &[DocNode],
  registry: &mut CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let style = &ctx.style.quote;

  // 段落先頭字下げは quotation のみ。本体は quote スタイルの font_kind を既定書体にする。
  let first_line_indent = if kind.indents_first_line() {
    style.first_line_indent
  } else {
    Length::pt(0.0)
  };
  let body_ctx = ctx.with_body_font_kind(style.font_kind).with_first_line_indent(first_line_indent);
  let children = lower_nodes_inner(&body_ctx, body, registry, headings)?;

  return Ok(vec![
    LayoutNode::Vkern {
      length: style.top_margin,
    },
    LayoutNode::VBox {
      children,
      margin_bottom: Length::pt(0.0),
      indent: style.indent,
      right_indent: style.indent,
      align: Align::Left,
    },
    LayoutNode::Vkern {
      length: style.bottom_margin,
    },
  ]);
}

#[cfg(test)]
mod tests {
  use config::read_style::Style as ReadStyle;
  use model::{DocNode, InlineNode, QuoteKind};

  use super::*;

  /// テキスト 1 段落の本体を作るヘルパ
  fn paragraph(text: &str) -> DocNode { return DocNode::Paragraph(vec![InlineNode::Text(text.to_string())]); }

  /// テスト用に新規 `CounterRegistry` / 見出し記録バッファを構築して `lower_quote` を呼ぶヘルパ
  fn lower_quote_default(
    ctx: &LoweringContext,
    kind: QuoteKind,
    body: &[DocNode],
  ) -> Result<Vec<LayoutNode>, LoweringError> {
    let mut registry = CounterRegistry::from_style(ctx.style);
    let mut headings = Vec::new();
    return lower_quote(ctx, kind, body, &mut registry, &mut headings);
  }

  /// `nodes` から本体 `VBox`（`indent` / `right_indent` / `children`）を取り出す
  fn body_vbox(nodes: &[LayoutNode]) -> (Length, Length, &[LayoutNode]) {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox {
          indent,
          right_indent,
          children,
          ..
        } => Some((*indent, *right_indent, children.as_slice())),
        _ => None,
      })
      .expect("本体 VBox があるはず");
  }

  #[test]
  fn quote_wraps_body_in_symmetric_indent_vbox_with_margins() {
    // Arrange — 既定スタイル
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quote, &[paragraph("body")]).expect("失敗しないはず");

    // Assert — 上下に Vkern、本体 VBox は左右とも style.quote.indent で字下げ
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は top_margin Vkern: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は bottom_margin Vkern: {nodes:?}");
    let (indent, right_indent, _) = body_vbox(&nodes);
    assert!((indent.to_pt() - style.quote.indent.to_pt()).abs() < f32::EPSILON);
    assert!((right_indent.to_pt() - style.quote.indent.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn quote_body_paragraph_has_no_first_line_indent_kern() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quote, &[paragraph("body")]).expect("失敗しないはず");

    // Assert — 本体に字下げ Kern は出ない
    let (_, _, children) = body_vbox(&nodes);
    assert!(
      !children.iter().any(|n| matches!(n, LayoutNode::Kern { .. })),
      "quote に字下げ Kern は出ない: {children:?}"
    );
    assert!(matches!(children.first(), Some(LayoutNode::Text(t, _)) if t == "body"));
  }

  #[test]
  fn quotation_body_paragraph_has_first_line_indent_kern() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quotation, &[paragraph("body")]).expect("失敗しないはず");

    // Assert — 本体先頭は first_line_indent 量の Kern
    let (_, _, children) = body_vbox(&nodes);
    let LayoutNode::Kern { length } = &children[0] else {
      panic!("quotation の本体先頭は字下げ Kern であるべき: {children:?}");
    };
    assert!((length.to_pt() - style.quote.first_line_indent.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn quote_body_uses_quote_style_font_kind() {
    // Arrange — quote スタイルの font_kind を本体の既定書体にする
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_quote_default(&ctx, QuoteKind::Quote, &[paragraph("body")]).expect("失敗しないはず");

    // Assert
    let (_, _, children) = body_vbox(&nodes);
    let body_kind = children.iter().find_map(|n| match n {
      LayoutNode::Text(t, s) if t == "body" => Some(s.font_kind),
      _ => None,
    });
    assert_eq!(body_kind, Some(style.quote.font_kind));
  }
}
