//! `"{number} {title}"` 形式テンプレートの `LayoutNode` 展開

use model::LabelId;

use super::{
  LoweringContext, LoweringState,
  inline::lower_inline,
  layout_node::{LayoutNode, TextStyle, merge_adjacent_text},
};
use crate::resolve::ResolvedInline;

/// `{number}` / `{title}` / `{of}` プレースホルダを持つテンプレートを `LayoutNode` 列に展開する
pub(super) fn expand_template(
  ctx: &LoweringContext,
  template: &str,
  number: &str,
  title: &[ResolvedInline],
  of: Option<&LabelId>,
  base_style: TextStyle,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
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
            nodes.extend(lower_inline(ctx, inline, base_style, state));
          }
        },
        "of" => {
          if let Some(target) = of {
            // `{of}` は proof の証明対象参照。従来どおりクリック不可のプレーンテキストとして
            // 埋め込む（`\ref` と違いリンク領域にはしない）ので、リテラル文字列として繋ぐ
            literal.push_str(&state.ref_display(ctx.style, target));
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
  return merge_adjacent_text(nodes);
}

/// 溜めたリテラル文字列を `Text` ノードとして書き出し、バッファを空にする
fn flush_literal(nodes: &mut Vec<LayoutNode>, literal: &mut String, style: TextStyle) {
  if literal.is_empty() {
    return;
  }
  nodes.push(LayoutNode::Text(std::mem::take(literal), style));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use model::{FontKind, Length};

  use super::{super::test_support, *};
  use crate::{
    config::{CounterName, Style as ReadStyle},
    resolve::{CounterKind, CounterValue},
  };

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
    let title = [ResolvedInline::Text(title_text.to_string())];
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    return expand_template(&ctx, template, number, &title, None, base_style(), &mut state);
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
      ResolvedInline::Text("A ".to_string()),
      ResolvedInline::Styled {
        kind: FontKind::SerifBold,
        children: vec![ResolvedInline::Text("B".to_string())],
      },
    ];
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);

    let nodes = expand_template(&ctx, "{number} {title}", "1", &title, None, base_style(), &mut state);

    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, s) if t == "1 A " && s.font_kind == FontKind::Serif));
    assert!(matches!(&nodes[1], LayoutNode::Text(t, s) if t == "B" && s.font_kind == FontKind::SerifBold));
  }

  #[test]
  fn inline_math_in_title_is_lowered() {
    use model::MathNode;
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::InlineMath(vec![MathNode::Text(
      "x".to_string(),
    )])];
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);

    let nodes = expand_template(&ctx, "{title}", "1", &title, None, base_style(), &mut state);

    let has_placeholder = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t.contains("[Math]")));
    assert!(!has_placeholder, "[Math] プレースホルダは出力されない: {nodes:?}");
    assert!(!nodes.is_empty());
  }

  #[test]
  fn ref_in_title_is_resolved_to_internal_link() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [ResolvedInline::Ref {
      target: model::LabelId::new("tab:one"),
      span: model::Span::DUMMY,
    }];
    let document = test_support::document(&[(
      "tab:one",
      CounterValue {
        kind: CounterKind::Counter(CounterName::Table),
        parts: vec![0, 1, 1],
      },
    )]);
    let mut state = LoweringState::new(&document);

    // Act
    let nodes = expand_template(&ctx, "{number} {title}", "1", &title, None, base_style(), &mut state);

    // Assert
    let link = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Link { target, children } => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, model::LinkTarget::Internal(model::AnchorId::Label(model::LabelId::new("tab:one"))));
    assert!(matches!(&link.1[0], LayoutNode::Text(t, _) if t == "Table 1.1"), "{:?}", link.1);
  }

  #[test]
  fn of_placeholder_is_resolved_into_surrounding_literal() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let document = test_support::document(&[(
      "thm:x",
      CounterValue {
        kind: CounterKind::Theorem(model::TheoremClass::Theorem),
        parts: vec![1],
      },
    )]);
    let mut state = LoweringState::new(&document);

    // Act
    let nodes =
      expand_template(&ctx, "Proof of {of}", "1", &[], Some(&model::LabelId::new("thm:x")), base_style(), &mut state);

    // Assert
    assert_eq!(nodes.len(), 1, "リンクにはせず前後のリテラルと 1 つの Text に繋がる: {nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "Proof of Theorem 1"), "{nodes:?}");
  }

  #[test]
  fn of_placeholder_omitted_when_none() {
    let nodes = expand_plain_with_of("Proof{of}", "1", None);

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "Proof"), "{nodes:?}");
  }

  /// `of` パラメータを明示できる `expand_plain` の派生ヘルパ
  fn expand_plain_with_of(template: &str, number: &str, of: Option<&LabelId>) -> Vec<LayoutNode> {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let document = test_support::document(&[]);
    let mut state = LoweringState::new(&document);
    return expand_template(&ctx, template, number, &[], of, base_style(), &mut state);
  }

  #[test]
  fn number_only_template_without_title_placeholder() {
    let nodes = expand_plain("No.{number}", "7", "Ignored");

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "No.7"));
  }
}
