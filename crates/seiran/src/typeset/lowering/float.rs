//! 図表（フロート）共通のキャプション構築と `VBox` 包み

use config::CaptionStyle;
use model::{CaptionPosition, FontKind, Length};

use super::{
  LoweringContext, LoweringState,
  layout_node::{LayoutNode, TextStyle},
  template::expand_template,
};
use crate::resolve::ResolvedInline;

/// キャプション本体（`format` テンプレの `{number}` / `{title}` を埋めた `LayoutNode` 列）を生成する
pub(super) fn build_caption(
  ctx: &LoweringContext,
  caption_style: &CaptionStyle,
  inlines: &[ResolvedInline],
  number: &str,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let base_style = TextStyle {
    font_size: caption_style.font_size,
    font_kind: FontKind::Serif,
    color: None,
  };
  return expand_template(ctx, &caption_style.format, number, inlines, None, base_style, state);
}

/// フロートの余白の指定
// 全フィールドが余白（*_margin）のみの構造体なので postfix 警告は意図どおり
#[allow(clippy::struct_field_names)]
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
      align: model::Align::Center,
    },
  ];
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use config::{CaptionStyle, CounterName, Style as ReadStyle};
  use model::{CaptionPosition, FontKind, Length};

  use super::{super::test_support, *};
  use crate::resolve::{CounterKind, CounterValue};

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

  /// 本体（main）として使う、キャプションと区別しやすい Rule ノードを作る
  fn main_node() -> LayoutNode {
    return LayoutNode::Rule {
      width: Length::pt(10.0),
      height: Length::pt(2.0),
    };
  }

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
    assert_eq!(*align, model::Align::Center, "図表は既定で中央寄せ");
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "cap"));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Rule { .. }));
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
    assert!(matches!(&children[0], LayoutNode::Rule { .. }));
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
    assert!(matches!(&children[0], LayoutNode::Rule { .. }));
  }

  #[test]
  fn build_caption_expands_template_with_serif_caption_style() {
    // Arrange
    let read_style = ReadStyle::default();
    let ctx = LoweringContext::new(&read_style);
    let caption_style = CaptionStyle {
      format: "Fig {number}: {title}".to_string(),
      font_size: Length::pt(9.0),
    };
    let inlines = [ResolvedInline::Text("Overview".to_string())];
    let document = test_support::document(&[]);

    // Act
    let nodes = build_caption(&ctx, &caption_style, &inlines, "3", &mut LoweringState::new(&document));

    // Assert
    assert_eq!(nodes.len(), 1, "プレーンタイトルは 1 つの Text に縮約される: {nodes:?}");
    let LayoutNode::Text(text, style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "Fig 3: Overview");
    assert_eq!(style.font_size, Length::pt(9.0));
    assert_eq!(style.font_kind, FontKind::Serif);
  }

  #[test]
  fn build_caption_ref_is_resolved_to_internal_link() {
    // Arrange
    let read_style = ReadStyle::default();
    let ctx = LoweringContext::new(&read_style);
    let caption_style = CaptionStyle::default();
    let inlines = [ResolvedInline::Ref {
      target: model::LabelId::new("fig:one"),
      span: model::Span::DUMMY,
    }];
    let document = test_support::document(&[(
      "fig:one",
      CounterValue {
        kind: CounterKind::Counter(CounterName::Figure),
        parts: vec![0, 1, 2],
      },
    )]);

    // Act
    let nodes = build_caption(&ctx, &caption_style, &inlines, "1", &mut LoweringState::new(&document));

    // Assert
    let link = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Link { target, children } => return Some((target, children)),
        _ => return None,
      })
      .expect("解決済み \\ref は Link になるはず");
    assert_eq!(*link.0, model::LinkTarget::Internal(model::AnchorId::Label(model::LabelId::new("fig:one"))));
    assert!(matches!(&link.1[0], LayoutNode::Text(t, _) if t == "Figure 1.2"), "{:?}", link.1);
  }
}
