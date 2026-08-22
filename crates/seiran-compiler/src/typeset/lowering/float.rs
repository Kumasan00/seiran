//! 図表（フロート）共通のキャプション構築と `VBox` 包み

use crate::{
  document::{CaptionPosition, FontKind, HirInline},
  length::Length,
  style::CaptionStyle,
  typeset::{
    boxes::Align,
    lowering::{
      LoweringContext, LoweringState,
      inline::lower_inlines,
      layout_node::{LayoutNode, TextStyle, merge_adjacent_text},
    },
  },
};

/// キャプション本体（`format` テンプレの `{number}` / `{title}` を埋めた `LayoutNode` 列）を生成する
///
/// キャプション本文の lowering はクロージャで遅延させ、`format` が `{title}` を含むときだけ
/// 含む回数ぶん実行する（キャプション中の `\footnote` が通し index だけ消費して消えるのを
/// 防ぐため。詳細は [`crate::style::NumberTitleTemplate::expand`] の doc コメント）。
pub(super) fn build_caption(
  ctx: &LoweringContext<'_>,
  caption_style: &CaptionStyle,
  inlines: &[HirInline],
  number: &str,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let base_style = TextStyle {
    font_size: caption_style.font_size,
    font_kind: FontKind::Serif,
    color: None,
  };
  let nodes = caption_style.format.expand(
    number,
    || return lower_inlines(ctx, inlines, base_style, state),
    |literal| return LayoutNode::Text(literal.to_string(), base_style),
  );
  return merge_adjacent_text(nodes);
}

/// フロートの余白の指定
#[expect(
  clippy::struct_field_names,
  reason = "全フィールドが余白（`*_margin`）で、postfix は種類ではなく長さの用途を表す"
)]
pub(super) struct FloatSpec {
  /// フロート全体の上マージン（VBox の前に Vkern として出力）
  pub top_margin: Length,
  /// フロート全体の下マージン（VBox の `margin_bottom`）
  pub bottom_margin: Length,
  /// 本体とキャプションの間に入れる余白（`Vkern` として出力。0pt なら実質アキなし）
  pub inner_margin: Length,
}

/// 本体とキャプションを `caption_position` の順序で積み、上下マージン付きの `VBox` で包む
pub(super) fn wrap_float(
  main: LayoutNode,
  caption: Option<(CaptionPosition, Vec<LayoutNode>)>,
  spec: &FloatSpec,
) -> Vec<LayoutNode> {
  let mut children = Vec::new();
  match caption {
    Some((CaptionPosition::Top, caption_nodes)) => {
      children.extend(caption_nodes);
      children.push(LayoutNode::Vkern {
        length: spec.inner_margin,
      });
      children.push(main);
    },
    Some((CaptionPosition::Bottom, caption_nodes)) => {
      children.push(main);
      children.push(LayoutNode::Vkern {
        length: spec.inner_margin,
      });
      children.extend(caption_nodes);
    },
    None => {
      children.push(main);
    },
  }

  return vec![
    LayoutNode::Vkern {
      length: spec.top_margin,
    },
    LayoutNode::VBox {
      children,
      margin_bottom: spec.bottom_margin,
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    },
  ];
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    semantics::LabelId,
    style::{CaptionStyle, NumberTitleTemplate, Style as ReadStyle},
    typeset::{
      boxes::{Align, AnchorId, LinkTarget},
      lowering::test_support::{analyzed, lower},
    },
  };

  /// `.sei` ソースを lower してレイアウトノード列を返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  /// フロート本体の `VBox`（画像を含む `VBox`）の子要素列を取り出すヘルパ
  fn float_body(nodes: &[LayoutNode]) -> &[LayoutNode] {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } if children.iter().any(|c| matches!(c, LayoutNode::Image { .. })) => {
          return Some(children.as_slice());
        },
        _ => return None,
      })
      .expect("画像を含む VBox があるはず");
  }

  /// テスト用のキャプション本体（識別しやすい固定文字列の Text）を作る
  fn caption_node(text: &str) -> LayoutNode {
    return LayoutNode::Text(
      text.to_string(),
      TextStyle {
        font_size: Length::pt(11.0),
        font_kind: FontKind::Serif,
        color: None,
      },
    );
  }

  /// 本体（main）として使う、キャプションと取り違えようのない固定文字列の Text を作る
  fn main_node() -> LayoutNode { return caption_node(MAIN_TEXT); }

  /// [`main_node`] が積む本文文字列（キャプションには現れない値）
  const MAIN_TEXT: &str = "MAIN";

  /// `LayoutNode` が指定 pt の `Vkern` であることを確認するヘルパ
  fn assert_vkern(node: &LayoutNode, expected_pt: f32) {
    let LayoutNode::Vkern { length } = node else {
      panic!("Vkern が期待されます: {node:?}");
    };
    assert!((length.to_pt() - expected_pt).abs() < f32::EPSILON, "Vkern={} 期待={expected_pt}", length.to_pt());
  }

  #[test]
  fn wrap_float_top_orders_caption_inner_kern_then_main() {
    // Arrange
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(3.0),
    };

    // Act
    let nodes = wrap_float(main_node(), Some((CaptionPosition::Top, vec![caption_node("cap")])), &spec);

    // Assert
    assert_eq!(nodes.len(), 2);
    assert_vkern(&nodes[0], 5.0);
    let LayoutNode::VBox {
      children,
      margin_bottom,
      align,
      ..
    } = &nodes[1]
    else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert!((margin_bottom.to_pt() - 7.0).abs() < f32::EPSILON);
    assert_eq!(*align, Align::Center, "図表は既定で中央寄せ");
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "cap"));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Text(t, _) if t == MAIN_TEXT));
  }

  #[test]
  fn wrap_float_bottom_orders_main_inner_kern_then_caption() {
    // Arrange
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(3.0),
    };

    // Act
    let nodes = wrap_float(main_node(), Some((CaptionPosition::Bottom, vec![caption_node("cap")])), &spec);

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == MAIN_TEXT));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Text(t, _) if t == "cap"));
  }

  #[test]
  fn wrap_float_zero_inner_margin_still_emits_harmless_vkern() {
    // Arrange
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(0.0),
    };

    // Act
    let nodes = wrap_float(main_node(), Some((CaptionPosition::Top, vec![caption_node("cap")])), &spec);

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert_eq!(children.len(), 3, "caption + Vkern(0) + main: {children:?}");
    assert_vkern(&children[1], 0.0);
  }

  #[test]
  fn wrap_float_without_caption_contains_only_main() {
    // Arrange
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(3.0),
    };

    // Act
    let nodes = wrap_float(main_node(), None, &spec);

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert_eq!(children.len(), 1, "本体のみ: {children:?}");
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == MAIN_TEXT));
  }

  #[test]
  fn build_caption_expands_template_with_serif_caption_style() {
    // Arrange
    let mut style = ReadStyle::default();
    style.figure.caption = CaptionStyle {
      format: NumberTitleTemplate::parse("Fig {number}: {title}"),
      font_size: Length::pt(9.0),
    };

    // Act
    let nodes =
      lower_source(&style, "\\chapter{C}\n\n\\begin{figure}\n\\image{a.png}\n\\caption{Overview}\n\\end{figure}\n");

    // Assert
    let caption = float_body(&nodes)
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(text, text_style) => return Some((text.clone(), *text_style)),
        _ => return None,
      })
      .expect("キャプション Text があるはず");
    assert_eq!(caption.0, "Fig 1.1: Overview");
    assert_eq!(caption.1.font_size, Length::pt(9.0));
    assert_eq!(caption.1.font_kind, FontKind::Serif);
  }

  #[test]
  fn caption_format_without_title_placeholder_does_not_consume_footnote_number() {
    // Arrange — `{title}` を含まない独自フォーマット（キャプション本文は一切表示されない）
    let mut style = ReadStyle::default();
    style.figure.caption = CaptionStyle {
      format: NumberTitleTemplate::parse("図 {number}"),
      font_size: Length::pt(9.0),
    };

    // Act
    let nodes = lower_source(
      &style,
      "\\chapter{C}\n\n\\begin{figure}\n\\image{a.png}\n\\caption{Overview\\footnote{in caption}}\n\\end{figure}\n\n\
       body\\footnote{in body}\n",
    );

    // Assert — キャプション本文を lower しないので、本文の脚注が 1 番のままになる
    let numbers: Vec<u32> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Footnote { number, .. } => return Some(*number),
        _ => return None,
      })
      .collect();
    assert_eq!(numbers, vec![1], "{nodes:?}");
  }

  #[test]
  fn build_caption_ref_is_resolved_to_internal_link() {
    // Arrange
    let style = ReadStyle::default();

    // Act — 2 枚目のキャプションから 1 枚目を `\ref` する
    let nodes = lower_source(
      &style,
      "\\chapter{C}\n\n\\begin{figure}[label=fig:one]\n\\image{a.png}\n\\caption{one}\n\\end{figure}\n\n\
       \\begin{figure}\n\\image{b.png}\n\\caption{\\ref{fig:one}}\n\\end{figure}\n",
    );

    // Assert
    let link = nodes
      .iter()
      .flat_map(|n| match n {
        LayoutNode::VBox { children, .. } => return children.as_slice(),
        _ => return &[] as &[LayoutNode],
      })
      .find_map(|n| match n {
        LayoutNode::Link { target, children } => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, LinkTarget::Internal(AnchorId::Label(LabelId::new("fig:one"))));
    assert!(matches!(&link.1[0], LayoutNode::Text(t, _) if t == "Figure 1.1"), "{:?}", link.1);
  }
}
