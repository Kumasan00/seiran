//! `"{number} {title}"` 形式テンプレートの `LayoutNode` 展開
//!
//! 見出し（`HeadingStyle.format`）とキャプション（`CaptionStyle.format`）が共用する。
//! `{title}` の中身はインライン要素のまま [`lower_inline`] で展開するため、
//! タイトル内の書体指定（`\bold` 等）やインライン数式が失われない。

use model::InlineNode;

use super::{
  LoweringContext, LoweringError,
  inline::lower_inline,
  layout_node::{LayoutNode, TextStyle, merge_adjacent_text},
};

/// `{number}` / `{title}` / `{of}` プレースホルダを持つテンプレートを `LayoutNode` 列に展開する
///
/// - リテラル部分と `{number}` は `base_style` の [`LayoutNode::Text`] として出力する
/// - `{title}` は各インライン要素を [`lower_inline`] で展開する（書体・数式を保持）
/// - `{of}`（`proof` の証明対象参照）は `of` が `Some((label, span))` のとき
///   [`LayoutNode::Ref`] プレースホルダを 1 つ出力する。`\ref` と同じく前方参照になり得るため
///   ここでは解決せず、pass2（[`super::resolve::resolve_refs`]）に委ねる。`None` なら何も出力しない
///   （旧 `of.unwrap_or("")` と同じ「証明対象なし」の空扱い）
/// - 未知のプレースホルダ・閉じ括弧の欠落はリテラル扱いで残す
/// - 出力前に隣接する同一スタイルの `Text` をマージするため、装飾なしタイトルは
///   単一の `Text` ノードに縮約される
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
            nodes.extend(lower_inline(ctx, inline, base_style)?);
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
  use config::read_style::Style as ReadStyle;
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
    return expand_template(&ctx, template, number, &title, None, base_style())
      .expect("プレーンタイトルは失敗しないはず");
  }

  #[test]
  fn plain_title_merges_into_single_text() {
    // 英語デフォルト: section は "{number} {title}"
    let nodes = expand_plain("{number} {title}", "2.3", "Intro");

    assert_eq!(nodes.len(), 1, "同一スタイルなので 1 つの Text に縮約される: {nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "2.3 Intro"));
  }

  #[test]
  fn japanese_decoration_template() {
    // 日本語化（style.toml 上書き例）が正しく置換されること
    let nodes = expand_plain("第{number}章 {title}", "3", "序論");

    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "第3章 序論"));
  }

  #[test]
  fn unknown_placeholders_are_literal() {
    // 未知プレースホルダと閉じ括弧の欠落はリテラルのまま残る
    let nodes = expand_plain("{foo} {number} {title} {bar", "1", "T");

    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "{foo} 1 T {bar"), "{nodes:?}");
  }

  #[test]
  fn styled_title_keeps_font_kind() {
    // タイトル内の \bold は base_style と異なるスタイルの Text として分離される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [
      InlineNode::Text("A ".to_string()),
      InlineNode::Styled {
        kind: FontKind::SerifBold,
        children: vec![InlineNode::Text("B".to_string())],
      },
    ];

    let nodes = expand_template(&ctx, "{number} {title}", "1", &title, None, base_style()).expect("失敗しないはず");

    // Text("1 A ", Serif) + Text("B", SerifBold)
    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, s) if t == "1 A " && s.font_kind == FontKind::Serif));
    assert!(matches!(&nodes[1], LayoutNode::Text(t, s) if t == "B" && s.font_kind == FontKind::SerifBold));
  }

  #[test]
  fn inline_math_in_title_is_lowered() {
    // タイトル内のインライン数式は "[Math]" プレースホルダではなく実ノードに展開される
    use model::MathNode;
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::InlineMath(vec![MathNode::Text(
      "x".to_string(),
    )])];

    let nodes = expand_template(&ctx, "{title}", "1", &title, None, base_style()).expect("失敗しないはず");

    let has_placeholder = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t.contains("[Math]")));
    assert!(!has_placeholder, "[Math] プレースホルダは出力されない: {nodes:?}");
    assert!(!nodes.is_empty());
  }

  #[test]
  fn ref_in_title_becomes_placeholder() {
    // {title} 内の \ref は即時解決せず LayoutNode::Ref プレースホルダとして埋め込まれる
    // （解決は pass2 = resolve::resolve_refs が担う）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::Ref {
      label: "tab:missing".to_string(),
      span: model::Span::DUMMY,
    }];

    let nodes =
      expand_template(&ctx, "{number} {title}", "1", &title, None, base_style()).expect("即時エラーにはならない");

    assert!(
      nodes.iter().any(|n| matches!(n, LayoutNode::Ref { label, .. } if label == "tab:missing")),
      "Ref プレースホルダが残るはず: {nodes:?}"
    );
  }

  #[test]
  fn of_placeholder_emits_ref_when_present() {
    // {of} は of が Some のとき LayoutNode::Ref プレースホルダを 1 つ出力する
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let of_span = model::Span::new(3, 4);

    let nodes =
      expand_template(&ctx, "Proof of {of}", "1", &[], Some(("thm:x", of_span)), base_style()).expect("失敗しないはず");

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
    // {of} は of が None のとき何も出力しない（旧 of.unwrap_or("") と同じ空扱い）
    let nodes = expand_plain_with_of("Proof{of}", "1", None);

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "Proof"), "{nodes:?}");
  }

  /// `of` パラメータを明示できる `expand_plain` の派生ヘルパ
  fn expand_plain_with_of(template: &str, number: &str, of: Option<(&str, model::Span)>) -> Vec<LayoutNode> {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    return expand_template(&ctx, template, number, &[], of, base_style()).expect("失敗しないはず");
  }

  #[test]
  fn number_only_template_without_title_placeholder() {
    // {title} を含まないテンプレートでは title は展開されず、{number} だけ置換される
    let nodes = expand_plain("No.{number}", "7", "Ignored");

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "No.7"));
  }
}
