//! `"{number} {title}"` 形式テンプレートの `LayoutNode` 展開

use super::layout_node::{LayoutNode, TextStyle, merge_adjacent_text};

/// `{number}` / `{title}` / `{of}` プレースホルダを持つテンプレートを `LayoutNode` 列に展開する
///
/// `of` は表示文字列化済みで受け取り、`title` は「呼び出したときに lowering 済みタイトルを返す
/// クロージャ」で受け取る（テンプレート展開自体は事実も可変状態も必要としないので、
/// `LoweringContext` / `LoweringState` は取らずクロージャの中に閉じ込める）。
///
/// `title` を値ではなくクロージャにしているのは、タイトルの lowering に副作用（`\footnote` の
/// 通し index の払い出し）があるためで、`{title}` プレースホルダの出現回数ぶんだけ、出現した
/// ときにだけ呼ぶ。すなわち:
///
/// - `{title}` を含まないテンプレート（例 `"図 {number}"`）ではタイトルを一度も lower しない。
///   捨てるだけのノードを作って脚注 index だけ消費し、以降の脚注番号をずらす事故を防ぐ。
/// - `{title}` を 2 回含むテンプレートでは 2 回 lower する。タイトル中の `\footnote` は
///   マーカーと本体が対になった別々の脚注として 2 個出る（clone して同じ index を共有させない）。
pub(super) fn expand_template(
  template: &str,
  number: &str,
  mut title: impl FnMut() -> Vec<LayoutNode>,
  of: Option<&str>,
  base_style: TextStyle,
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
          nodes.extend(title());
        },
        "of" => {
          if let Some(display) = of {
            // `{of}` は proof の証明対象参照。従来どおりクリック不可のプレーンテキストとして
            // 埋め込む（`\ref` と違いリンク領域にはしない）ので、リテラル文字列として繋ぐ
            literal.push_str(display);
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
  use super::*;
  use crate::{font::FontKind, length::Length};

  /// テンプレート展開の基底スタイル
  fn base_style() -> TextStyle {
    return TextStyle {
      font_size: Length::pt(10.0),
      font_kind: FontKind::Serif,
      color: None,
    };
  }

  /// 基底スタイルのプレーンなタイトルノード 1 個を作る
  fn plain_title(text: &str) -> Vec<LayoutNode> { return vec![LayoutNode::Text(text.to_string(), base_style())]; }

  /// プレーンタイトルでテンプレ展開するヘルパ
  fn expand_plain(template: &str, number: &str, title_text: &str) -> Vec<LayoutNode> {
    return expand_template(template, number, || return plain_title(title_text), None, base_style());
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
    // Arrange — 呼び出し元が lower 済みのタイトル（書体切り替えを含む）を渡す
    let bold = TextStyle {
      font_kind: FontKind::SerifBold,
      ..base_style()
    };
    let title = vec![
      LayoutNode::Text("A ".to_string(), base_style()),
      LayoutNode::Text("B".to_string(), bold),
    ];

    // Act
    let nodes = expand_template("{number} {title}", "1", || return title.clone(), None, base_style());

    // Assert
    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, s) if t == "1 A " && s.font_kind == FontKind::Serif));
    assert!(matches!(&nodes[1], LayoutNode::Text(t, s) if t == "B" && s.font_kind == FontKind::SerifBold));
  }

  #[test]
  fn non_text_title_nodes_are_passed_through() {
    // Arrange — インライン数式のように Text 以外へ落ちるタイトルもそのまま挿し込む
    let title = vec![LayoutNode::Raise {
      offset: Length::pt(1.0),
      children: vec![LayoutNode::Text("x".to_string(), base_style())],
    }];

    // Act
    let nodes = expand_template("{title}", "1", || return title.clone(), None, base_style());

    // Assert
    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Raise { .. }), "{nodes:?}");
  }

  #[test]
  fn of_placeholder_is_resolved_into_surrounding_literal() {
    // Act — `{of}` は表示文字列化済みで渡ってくる
    let nodes = expand_template("Proof of {of}", "1", Vec::new, Some("Theorem 1"), base_style());

    // Assert
    assert_eq!(nodes.len(), 1, "リンクにはせず前後のリテラルと 1 つの Text に繋がる: {nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "Proof of Theorem 1"), "{nodes:?}");
  }

  #[test]
  fn of_placeholder_omitted_when_none() {
    let nodes = expand_template("Proof{of}", "1", Vec::new, None, base_style());

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "Proof"), "{nodes:?}");
  }

  #[test]
  fn number_only_template_without_title_placeholder() {
    let nodes = expand_plain("No.{number}", "7", "Ignored");

    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "No.7"));
  }

  #[test]
  fn title_closure_is_not_called_without_title_placeholder() {
    // Arrange — タイトルの lowering には副作用があるので、出現しないなら呼んではならない
    let mut calls = 0_u32;

    // Act
    let nodes = expand_template(
      "No.{number}",
      "7",
      || {
        calls += 1;
        return plain_title("Ignored");
      },
      None,
      base_style(),
    );

    // Assert
    assert_eq!(calls, 0, "{nodes:?}");
  }

  #[test]
  fn title_closure_is_called_once_per_placeholder_occurrence() {
    // Arrange
    let mut calls = 0_u32;

    // Act
    let nodes = expand_template(
      "{title} / {title}",
      "7",
      || {
        calls += 1;
        return plain_title(&format!("T{calls}"));
      },
      None,
      base_style(),
    );

    // Assert — 2 回とも別々に lower される（clone で同じノードを 2 つ置くのではない）
    assert_eq!(calls, 2);
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "T1 / T2"), "{nodes:?}");
  }
}
