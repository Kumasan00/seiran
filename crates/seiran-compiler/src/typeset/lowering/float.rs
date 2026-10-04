//! 図表（フロート）共通の lowering 経路
//!
//! 採番されるフロート（図・表）の「カウンタ値 → 番号文字列 → 本体 → キャプション → 上下マージン付き
//! `VBox` → ラベルアンカー」を [`lower_numbered_float`] 1 本に持ち、図と表は本体ノードの作り方と
//! 体裁（[`FloatCaption`] / [`FloatMargins`]）だけを渡す。

use crate::{
  document::{CaptionPosition, HirInline, NodeId},
  length::Length,
  style::{CaptionStyle, TextAlignment},
  typeset::lowering::{
    LoweringContext, LoweringState, counter,
    inline::lower_inlines,
    layout_node::{InlineNode, LayoutNode, TextStyle, merge_adjacent_text},
    with_label_anchors,
  },
};

/// キャプション本体（`format` テンプレの `{number}` / `{title}` を埋めた `InlineNode` 列）を生成する
fn build_caption(
  ctx: &LoweringContext<'_>,
  caption_style: &CaptionStyle,
  inlines: &[HirInline],
  number: &str,
  state: &mut LoweringState<'_>,
) -> Vec<InlineNode> {
  let base_style = TextStyle {
    font_size: caption_style.font_size,
    typeface: caption_style.typeface,
    color: None,
  };
  let nodes = caption_style.format.expand(
    number,
    || return lower_inlines(ctx, inlines, base_style, state),
    |literal| return InlineNode::Text(literal.to_string(), base_style),
  );
  return merge_adjacent_text(nodes);
}

/// フロート 1 件の上下の余白と、本体・キャプション間の余白
pub(super) struct FloatMargins {
  /// フロート全体の上マージン（VBox の前に Vkern として出力）
  pub top: Length,
  /// フロート全体の下マージン（VBox の `margin_bottom`）
  pub bottom: Length,
  /// 本体とキャプションの間に入れる余白（`Vkern` として出力）
  pub inner: Length,
}

/// 本体とキャプションを `caption_position` の順序で積み、上下マージン付きの `VBox` で包む
fn wrap_float(
  main: LayoutNode,
  caption: Option<(CaptionPosition, Vec<InlineNode>)>,
  margins: &FloatMargins,
) -> Vec<LayoutNode> {
  let mut children = Vec::new();
  match caption {
    Some((CaptionPosition::Top, caption_nodes)) => {
      children.extend(caption_nodes.into_iter().map(LayoutNode::from));
      children.push(LayoutNode::Vkern {
        length: margins.inner,
      });
      children.push(main);
    },
    Some((CaptionPosition::Bottom, caption_nodes)) => {
      children.push(main);
      children.push(LayoutNode::Vkern {
        length: margins.inner,
      });
      children.extend(caption_nodes.into_iter().map(LayoutNode::from));
    },
    None => {
      children.push(main);
    },
  }

  return vec![
    LayoutNode::Vkern {
      length: margins.top,
    },
    LayoutNode::VBox {
      children,
      margin_bottom: margins.bottom,
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: Some(TextAlignment::Center),
    },
  ];
}

/// フロート 1 件のキャプション指定（体裁・本文・位置）
///
/// `inlines` が `None` なら `position` は読まれない。
#[derive(Debug, Clone, Copy)]
pub(super) struct FloatCaption<'a> {
  /// キャプションの体裁（`style.figure.caption` / `style.table.caption`）
  pub style: &'a CaptionStyle,
  /// キャプション本文のインライン列。`\caption` が無ければ `None`
  pub inlines: Option<&'a [HirInline]>,
  /// キャプションを本体の上下どちらに置くか
  pub position: CaptionPosition,
}

/// 採番されるフロート（図・表）1 件をレイアウトノード列に変換する
///
/// `build_body` を `build_caption` より先に呼ぶのは、表セルの `\footnote` が
/// キャプションの `\footnote` より先に通し番号を取る本文の出現順を保つため。
pub(super) fn lower_numbered_float(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  caption: FloatCaption<'_>,
  margins: &FloatMargins,
  state: &mut LoweringState<'_>,
  build_body: impl FnOnce(&mut LoweringState<'_>) -> LayoutNode,
) -> Vec<LayoutNode> {
  let Some(counter_value) = state.counter_value(id) else {
    unreachable!("図表は必ず採番される（analyze の Figure / Table 分岐が counters へ登録している）: {id:?}")
  };
  let number = counter::format_counter_value(ctx.style, counter_value);
  let label = state.declared_label(id);

  let body = build_body(state);
  let caption_nodes = caption
    .inlines
    .map(|inlines| return (caption.position, build_caption(ctx, caption.style, inlines, &number, state)));

  return with_label_anchors(label, wrap_float(body, caption_nodes, margins));
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    document::Typeface,
    semantics::LabelId,
    style::{CaptionStyle, NumberTitleTemplate, Style as ReadStyle, TextAlignment},
    typeset::{
      boxes::{AnchorId, LinkTarget},
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
  fn caption_node(text: &str) -> InlineNode {
    return InlineNode::Text(
      text.to_string(),
      TextStyle {
        font_size: Length::pt(11.0),
        typeface: Typeface::Serif,
        color: None,
      },
    );
  }

  /// 本体（main）として使う、キャプションと取り違えようのない固定文字列の Text を作る
  fn main_node() -> LayoutNode { return LayoutNode::from(caption_node(MAIN_TEXT)); }

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
    let margins = FloatMargins {
      top: Length::pt(5.0),
      bottom: Length::pt(7.0),
      inner: Length::pt(3.0),
    };

    let nodes = wrap_float(main_node(), Some((CaptionPosition::Top, vec![caption_node("cap")])), &margins);

    assert_eq!(nodes.len(), 2);
    assert_vkern(&nodes[0], 5.0);
    let LayoutNode::VBox {
      children,
      margin_bottom,
      alignment,
      ..
    } = &nodes[1]
    else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert!((margin_bottom.to_pt() - 7.0).abs() < f32::EPSILON);
    assert_eq!(*alignment, Some(TextAlignment::Center), "図表は中央寄せ固定");
    assert_eq!(children.len(), 3, "caption + Vkern + main: {children:?}");
    assert!(matches!(&children[0], LayoutNode::Inline(InlineNode::Text(t, _)) if t == "cap"));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Inline(InlineNode::Text(t, _)) if t == MAIN_TEXT));
  }

  #[test]
  fn wrap_float_bottom_orders_main_inner_kern_then_caption() {
    let margins = FloatMargins {
      top: Length::pt(5.0),
      bottom: Length::pt(7.0),
      inner: Length::pt(3.0),
    };

    let nodes = wrap_float(main_node(), Some((CaptionPosition::Bottom, vec![caption_node("cap")])), &margins);

    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert!(matches!(&children[0], LayoutNode::Inline(InlineNode::Text(t, _)) if t == MAIN_TEXT));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Inline(InlineNode::Text(t, _)) if t == "cap"));
  }

  #[test]
  fn wrap_float_without_caption_contains_only_main() {
    let margins = FloatMargins {
      top: Length::pt(5.0),
      bottom: Length::pt(7.0),
      inner: Length::pt(3.0),
    };

    let nodes = wrap_float(main_node(), None, &margins);

    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert_eq!(children.len(), 1, "本体のみ: {children:?}");
    assert!(matches!(&children[0], LayoutNode::Inline(InlineNode::Text(t, _)) if t == MAIN_TEXT));
  }

  #[test]
  fn build_caption_expands_template_with_default_caption_typeface() {
    let mut style = ReadStyle::default();
    style.figure.caption = CaptionStyle {
      format: NumberTitleTemplate::parse("Fig {number}: {title}"),
      font_size: Length::pt(9.0),
      ..CaptionStyle::default()
    };

    let nodes =
      lower_source(&style, "\\chapter{C}\n\n\\begin{figure}\n\\image{a.png}\n\\caption{Overview}\n\\end{figure}\n");

    let caption = float_body(&nodes)
      .iter()
      .find_map(|n| match n {
        LayoutNode::Inline(InlineNode::Text(text, text_style)) => return Some((text.clone(), *text_style)),
        _ => return None,
      })
      .expect("キャプション Text があるはず");
    assert_eq!(caption.0, "Fig 1.1: Overview");
    assert_eq!(caption.1.font_size, Length::pt(9.0));
    assert_eq!(caption.1.typeface, Typeface::Serif);
  }

  #[test]
  fn build_caption_follows_style_caption_typeface() {
    let mut style = ReadStyle::default();
    style.figure.caption.typeface = Typeface::SansSerif;

    let nodes =
      lower_source(&style, "\\chapter{C}\n\n\\begin{figure}\n\\image{a.png}\n\\caption{Overview}\n\\end{figure}\n");

    let captions: Vec<(String, TextStyle)> = float_body(&nodes)
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Inline(InlineNode::Text(text, text_style)) => return Some((text.clone(), *text_style)),
        _ => return None,
      })
      .collect();
    assert_eq!(captions.len(), 1, "番号部分と本文部分は同じ書体なので 1 個の Text に併合される: {captions:?}");
    assert_eq!(captions[0].0, "Figure 1.1: Overview");
    assert_eq!(captions[0].1.typeface, Typeface::SansSerif);
  }

  #[test]
  fn caption_format_without_title_placeholder_does_not_consume_footnote_number() {
    let mut style = ReadStyle::default();
    style.figure.caption = CaptionStyle {
      format: NumberTitleTemplate::parse("図 {number}"),
      font_size: Length::pt(9.0),
      ..CaptionStyle::default()
    };

    let nodes = lower_source(
      &style,
      "\\chapter{C}\n\n\\begin{figure}\n\\image{a.png}\n\\caption{Overview\\footnote{in caption}}\n\\end{figure}\n\n\
       body\\footnote{in body}\n",
    );

    let numbers: Vec<u32> = nodes
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Inline(InlineNode::Footnote { number, .. }) => return Some(*number),
        _ => return None,
      })
      .collect();
    assert_eq!(numbers, vec![1], "{nodes:?}");
  }

  #[test]
  fn build_caption_ref_is_resolved_to_internal_link() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\chapter{C}\n\n\\begin{figure}[label=fig:one]\n\\image{a.png}\n\\caption{one}\n\\end{figure}\n\n\
       \\begin{figure}\n\\image{b.png}\n\\caption{\\ref{fig:one}}\n\\end{figure}\n",
    );

    let link = nodes
      .iter()
      .flat_map(|n| match n {
        LayoutNode::VBox { children, .. } => return children.as_slice(),
        _ => return &[],
      })
      .find_map(|n| match n {
        LayoutNode::Inline(InlineNode::Link { target, children }) => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, LinkTarget::Internal(AnchorId::Label(LabelId::new("fig:one"))));
    assert!(matches!(&link.1[0], InlineNode::Text(t, _) if t == "Figure 1.1"), "{:?}", link.1);
  }
}
