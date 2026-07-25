//! `"{number} {title}"` 形式テンプレートの `LayoutNode` 展開

use model::InlineNode;

use super::{
  LoweringContext, LoweringError,
  counter::CounterRegistry,
  inline::lower_inline,
  layout_node::{LayoutNode, TextStyle, merge_adjacent_text},
};

/// `{number}` / `{title}` / `{of}` プレースホルダを持つテンプレートを `LayoutNode` 列に展開する
///
/// # Errors
///
/// `{title}` 内に未解決の `\ref` がある場合に [`LoweringError::UnresolvedReference`] を返します。
pub(super) fn expand_template(
  ctx: &LoweringContext,
  template: &str,
  number: &str,
  title: &[InlineNode],
  of: Option<(&str, model::Span)>,
  base_style: TextStyle,
  registry: &mut CounterRegistry,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut nodes: Vec<LayoutNode> = Vec::new();
  let mut literal = String::new();
  for segment in super::placeholder::segments(template) {
    match segment {
      super::placeholder::Segment::Literal(s) => literal.push_str(s),
      super::placeholder::Segment::Placeholder(name) => match name {
        "number" => literal.push_str(number),
        "title" => {
          flush_literal(&mut nodes, &mut literal, base_style);
          for inline in title {
            nodes.extend(lower_inline(ctx, inline, base_style, registry)?);
          }
        },
        "of" => {
          if let Some((label, span)) = of {
            flush_literal(&mut nodes, &mut literal, base_style);
            // `{of}` は proof の証明対象参照。従来どおりクリック不可のプレーンテキストとして
            // 埋め込む（`\ref` と違いリンク領域にはしない）
            nodes.push(LayoutNode::Ref {
              label: label.to_string(),
              span: super::span_to_source_span(span),
              style: base_style,
              as_link: false,
              source: ctx.source,
            });
          }
        },
        _ => {
          // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
          literal.push('{');
          literal.push_str(name);
          literal.push('}');
        },
      },
    }
  }
  flush_literal(&mut nodes, &mut literal, base_style);
  return Ok(merge_adjacent_text(nodes));
}

/// 溜めたリテラル文字列を `Text` ノードとして書き出し、バッファを空にする
fn flush_literal(nodes: &mut Vec<LayoutNode>, literal: &mut String, style: TextStyle) {
  if literal.is_empty() {
    return;
  }
  nodes.push(LayoutNode::Text(std::mem::take(literal), style));
}

#[cfg(test)]
mod tests {
  use config::Style as ReadStyle;
  use model::{FontKind, Length};

  use super::*;

  fn base_style() -> TextStyle {
    return TextStyle {
      font_size: Length::pt(10.0),
      font_kind: FontKind::Serif,
      color: None,
    };
  }

  /// プレーンタイトルでテンプレ展開し、単一 Text の文字列を取り出すヘルパ
  fn expand_plain(template: &str, number: &str, title_text: &str) -> Vec<LayoutNode> {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::Text(title_text.to_string())];
    return expand_template(
      &ctx,
      template,
      number,
      &title,
      None,
      base_style(),
      &mut CounterRegistry::default_for_seiran(),
    )
    .expect("プレーンタイトルは失敗しないはず");
  }

  #[test]
  fn plain_title_merges_into_single_text() {
    let nodes = expand_plain("{number} {title}", "2.3", "Intro");

    assert_eq!(nodes.len(), 1, "同一スタイルなので 1 つの Text に縮約される: {nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "2.3 Intro"));
  }

  #[test]
  fn japanese_decoration_template() {
    let nodes = expand_plain("第{number}章 {title}", "3", "序論");

    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "第3章 序論"));
  }

  #[test]
  fn unknown_placeholders_are_literal() {
    let nodes = expand_plain("{foo} {number} {title} {bar", "1", "T");

    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "{foo} 1 T {bar"), "{nodes:?}");
  }

  #[test]
  fn styled_title_keeps_font_kind() {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [
      InlineNode::Text("A ".to_string()),
      InlineNode::Styled {
        kind: FontKind::SerifBold,
        children: vec![InlineNode::Text("B".to_string())],
      },
    ];

    let nodes = expand_template(
      &ctx,
      "{number} {title}",
      "1",
      &title,
      None,
      base_style(),
      &mut CounterRegistry::default_for_seiran(),
    )
    .expect("失敗しないはず");

    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, s) if t == "1 A " && s.font_kind == FontKind::Serif));
    assert!(matches!(&nodes[1], LayoutNode::Text(t, s) if t == "B" && s.font_kind == FontKind::SerifBold));
  }

  #[test]
  fn inline_math_in_title_is_lowered() {
    use model::MathNode;
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::InlineMath(vec![MathNode::Text(
      "x".to_string(),
    )])];

    let nodes =
      expand_template(&ctx, "{title}", "1", &title, None, base_style(), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しないはず");

    let has_placeholder = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t.contains("[Math]")));
    assert!(!has_placeholder, "[Math] プレースホルダは出力されない: {nodes:?}");
    assert!(!nodes.is_empty());
  }

  #[test]
  fn ref_in_title_becomes_placeholder() {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::Ref {
      label: "tab:missing".to_string(),
      span: model::Span::DUMMY,
    }];

    let nodes = expand_template(
      &ctx,
      "{number} {title}",
      "1",
      &title,
      None,
      base_style(),
      &mut CounterRegistry::default_for_seiran(),
    )
    .expect("即時エラーにはならない");

    assert!(
      nodes.iter().any(|n| matches!(n, LayoutNode::Ref { label, .. } if label == "tab:missing")),
      "Ref プレースホルダが残るはず: {nodes:?}"
    );
  }

  #[test]
  fn of_placeholder_emits_ref_when_present() {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let of_span = model::Span::new(3, 4);

    let nodes = expand_template(
      &ctx,
      "Proof of {of}",
      "1",
      &[],
      Some(("thm:x", of_span)),
      base_style(),
      &mut CounterRegistry::default_for_seiran(),
    )
    .expect("失敗しないはず");

    let expected_span = super::super::span_to_source_span(of_span);
    assert!(
      nodes
        .iter()
        .any(|n| matches!(n, LayoutNode::Ref { label, span, .. } if label == "thm:x" && *span == expected_span)),
      "of の Ref プレースホルダが出るはず: {nodes:?}"
    );
  }

  #[test]
  fn of_placeholder_omitted_when_none() {
    let nodes = expand_plain_with_of("Proof{of}", "1", None);

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "Proof"), "{nodes:?}");
  }

  /// `of` パラメータを明示できる `expand_plain` の派生ヘルパ
  fn expand_plain_with_of(template: &str, number: &str, of: Option<(&str, model::Span)>) -> Vec<LayoutNode> {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    return expand_template(&ctx, template, number, &[], of, base_style(), &mut CounterRegistry::default_for_seiran())
      .expect("失敗しないはず");
  }

  #[test]
  fn number_only_template_without_title_placeholder() {
    let nodes = expand_plain("No.{number}", "7", "Ignored");

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "No.7"));
  }
}
