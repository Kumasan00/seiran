//! `"{number} {title}"` 形式テンプレートの `LayoutNode` 展開
//!
//! 見出し（`HeadingStyle.format`）とキャプション（`CaptionStyle.format`）が共用する。
//! `{title}` の中身はインライン要素のまま [`lower_inline`] で展開するため、
//! タイトル内の書体指定（`\bold` 等）やインライン数式が失われない。

use parser::document::InlineNode;

use super::{LoweringContext, LoweringError, inline::lower_inline};
use crate::layout_node::{LayoutNode, TextStyle};

/// `{number}` / `{title}` プレースホルダを持つテンプレートを `LayoutNode` 列に展開する
///
/// - リテラル部分と `{number}` は `base_style` の [`LayoutNode::Text`] として出力する
/// - `{title}` は各インライン要素を [`lower_inline`] で展開する（書体・数式を保持）
/// - 未知のプレースホルダ・閉じ括弧の欠落はリテラル扱いで残す
///   （`parser::evaluator::counter` のテンプレート展開と同じ方針）
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
  base_style: TextStyle,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut nodes: Vec<LayoutNode> = Vec::new();
  let mut literal = String::new();
  let mut chars = template.chars().peekable();
  while let Some(c) = chars.next() {
    if c != '{' {
      literal.push(c);
      continue;
    }
    let mut name = String::new();
    let mut closed = false;
    while let Some(&nc) = chars.peek() {
      chars.next();
      if nc == '}' {
        closed = true;
        break;
      }
      name.push(nc);
    }
    if !closed {
      // 閉じ括弧なしの `{...` はリテラル扱いとして残す
      literal.push('{');
      literal.push_str(&name);
      continue;
    }
    match name.as_str() {
      "number" => literal.push_str(number),
      "title" => {
        flush_literal(&mut nodes, &mut literal, base_style);
        for inline in title {
          nodes.extend(lower_inline(ctx, inline, base_style)?);
        }
      },
      _ => {
        // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
        literal.push('{');
        literal.push_str(&name);
        literal.push('}');
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

/// 隣接する同一スタイルの `Text` ノードを 1 つに結合する
fn merge_adjacent_text(nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  let mut out: Vec<LayoutNode> = Vec::with_capacity(nodes.len());
  for node in nodes {
    match (out.last_mut(), node) {
      (Some(LayoutNode::Text(prev, prev_style)), LayoutNode::Text(cur, cur_style)) if *prev_style == cur_style => {
        prev.push_str(&cur);
      },
      (_, node) => out.push(node),
    }
  }
  return out;
}

#[cfg(test)]
mod tests {
  use read_style::Style as ReadStyle;
  use types::FontKind;

  use super::*;

  fn base_style() -> TextStyle {
    return TextStyle {
      font_size: 10.0,
      font_kind: FontKind::Serif,
    };
  }

  /// プレーンタイトルでテンプレ展開し、単一 Text の文字列を取り出すヘルパ
  fn expand_plain(template: &str, number: &str, title_text: &str) -> Vec<LayoutNode> {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::Text(title_text.to_string())];
    return expand_template(&ctx, template, number, &title, base_style()).expect("プレーンタイトルは失敗しないはず");
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

    let nodes = expand_template(&ctx, "{number} {title}", "1", &title, base_style()).expect("失敗しないはず");

    // Text("1 A ", Serif) + Text("B", SerifBold)
    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, s) if t == "1 A " && s.font_kind == FontKind::Serif));
    assert!(matches!(&nodes[1], LayoutNode::Text(t, s) if t == "B" && s.font_kind == FontKind::SerifBold));
  }

  #[test]
  fn inline_math_in_title_is_lowered() {
    // タイトル内のインライン数式は "[Math]" プレースホルダではなく実ノードに展開される
    use parser::document::MathNode;
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [InlineNode::InlineMath(vec![MathNode::Text(
      "x".to_string(),
    )])];

    let nodes = expand_template(&ctx, "{title}", "1", &title, base_style()).expect("失敗しないはず");

    let has_placeholder = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t.contains("[Math]")));
    assert!(!has_placeholder, "[Math] プレースホルダは出力されない: {nodes:?}");
    assert!(!nodes.is_empty());
  }
}
