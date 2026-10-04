//! 図環境（`document::HirNodeKind::Figure`）の lowering

use crate::{
  document::{HirFigure, NodeId},
  typeset::lowering::{
    LoweringContext, LoweringState,
    float::{FloatCaption, FloatMargins, lower_numbered_float},
    layout_node::LayoutNode,
  },
};

/// 図をレイアウトノードに変換する
pub(super) fn lower_figure(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  figure: &HirFigure,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let style = &ctx.style.figure;

  let downsample_enabled = figure.downsample.unwrap_or(ctx.image_downsample);
  let target_dpi = if downsample_enabled {
    Some(figure.dpi.unwrap_or(ctx.image_max_dpi))
  } else {
    None
  };

  let margins = FloatMargins {
    top: style.top_margin,
    bottom: style.bottom_margin,
    inner: style.inner_margin,
  };
  let caption = FloatCaption {
    style: &style.caption,
    inlines: figure.caption.as_deref(),
    position: figure.caption_position,
  };
  return lower_numbered_float(ctx, id, caption, &margins, state, |_state| {
    // 画像ノードの構築は状態に触らない（`\image` の中にインラインは入らない）。
    return LayoutNode::Image {
      path: figure.image_path.clone(),
      width: figure.width,
      height: figure.height,
      target_dpi,
    };
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    length::Length,
    project::config::ImageConfig,
    style::Style as ReadStyle,
    typeset::lowering::{
      layout_node::InlineNode,
      lower_sources_with_headings,
      test_support::{analyzed, context},
    },
  };

  /// `.sei` ソースを与えられた文脈で lower するテストヘルパ
  fn lower_source(ctx: &LoweringContext<'_>, source: &str) -> Vec<LayoutNode> {
    let (layout, _headings) = lower_sources_with_headings(ctx, &analyzed(source));
    return layout;
  }

  /// 図の本体 `VBox`（画像を含む `VBox`）の子要素列を取り出す
  fn figure_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
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

  #[test]
  fn lower_figure_emits_image_and_caption_in_bottom_order() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(
      &ctx,
      "\\chapter{C}\n\n\\begin{figure}\n\\image[width=80mm, height=60mm]{./images/seiran.jpg}\n\\caption{せいらん}\n\\end{figure}\n",
    );

    let body_idx = nodes
      .iter()
      .position(|n| matches!(n, LayoutNode::VBox { children, .. } if children.iter().any(|c| matches!(c, LayoutNode::Image { .. }))))
      .expect("画像を含む VBox があるはず");
    assert!(matches!(nodes.get(body_idx - 1), Some(LayoutNode::Vkern { .. })), "{nodes:?}");
    let children = figure_children(&nodes);
    let LayoutNode::Image {
      path,
      width,
      height,
      target_dpi,
    } = children.first().expect("先頭は画像")
    else {
      panic!("先頭は Image であるべき: {children:?}");
    };
    assert_eq!(path.to_string(), "./images/seiran.jpg");
    assert!((width.expect("width 指定あり").to_pt() - Length::mm(80.0).to_pt()).abs() < 0.01);
    assert!((height.expect("height 指定あり").to_pt() - Length::mm(60.0).to_pt()).abs() < 0.01);
    assert_eq!(*target_dpi, Some(300));

    let caption_text = children.iter().find_map(|n| match n {
      LayoutNode::Inline(InlineNode::Text(text, _)) => return Some(text.as_str()),
      _ => return None,
    });
    assert_eq!(caption_text, Some("Figure 1.1: せいらん"));
  }

  #[test]
  fn lower_figure_caption_position_top_swaps_order() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(
      &ctx,
      "\\begin{figure}\n\\caption{せいらん}\n\\image[width=10mm, height=10mm]{a.png}\n\\end{figure}\n",
    );

    let children = figure_children(&nodes);
    let first_text_idx = children
      .iter()
      .position(|n| matches!(n, LayoutNode::Inline(InlineNode::Text(_, _))))
      .expect("Text あり");
    let first_image_idx = children.iter().position(|n| matches!(n, LayoutNode::Image { .. })).expect("Image あり");
    assert!(first_text_idx < first_image_idx, "Top: caption が image の前");
  }

  #[test]
  fn lower_figure_without_caption_omits_caption_node() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[width=10mm, height=10mm]{a.png}\n\\end{figure}\n");

    let children = figure_children(&nodes);
    let has_text = children.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Text(_, _))));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
    let has_image = children.iter().any(|n| matches!(n, LayoutNode::Image { .. }));
    assert!(has_image, "画像は出力されている: {children:?}");
  }

  #[test]
  fn lower_figure_per_image_downsample_false_yields_no_target_dpi() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[downsample=false]{a.png}\n\\end{figure}\n");

    let LayoutNode::Image { target_dpi, .. } = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }

  #[test]
  fn lower_figure_per_image_dpi_overrides_style() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[dpi=600]{a.png}\n\\end{figure}\n");

    let LayoutNode::Image { target_dpi, .. } = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert_eq!(*target_dpi, Some(600));
  }

  #[test]
  fn lower_figure_style_downsample_false_yields_no_target_dpi() {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(
      &style,
      ImageConfig {
        max_dpi: 300,
        downsample: false,
      },
    );

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image{a.png}\n\\end{figure}\n");

    let LayoutNode::Image { target_dpi, .. } = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }
}
