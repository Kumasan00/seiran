//! 図環境（`resolve::ResolvedNode::Figure`）の lowering

use super::{
  LoweringContext, LoweringState,
  float::{FloatSpec, build_caption, wrap_float},
  layout_node::LayoutNode,
};
use crate::{
  model::{AssetId, CaptionPosition, Length},
  resolve::ResolvedInline,
};

/// `\image` の per-image 上書き引数（dpi / downsample）を 1 つにまとめた構造体
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ImageOverrides {
  /// `\image[dpi=N]` の per-image 上書き。`None` なら config `[image].max_dpi`（既定）が使われる
  pub dpi: Option<u32>,
  /// `\image[downsample=true|false]` の per-image 上書き。`None` なら config `[image].downsample`（既定）が使われる
  pub downsample: Option<bool>,
}

/// 図をレイアウトノードに変換する
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_figure(
  ctx: &LoweringContext,
  image_path: &AssetId,
  width: Option<Length>,
  height: Option<Length>,
  overrides: ImageOverrides,
  caption: Option<(CaptionPosition, &[ResolvedInline])>,
  number: &str,
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let style = &ctx.style.figure;

  // ダウンサンプリングの既定（max_dpi / downsample）は出力物理の設定で config `[image]` 由来。
  // per-image の `\image[dpi=...]` / `[downsample=...]` 上書きが優先される。
  let downsample_enabled = overrides.downsample.unwrap_or(ctx.image_downsample);
  let target_dpi = if downsample_enabled {
    Some(overrides.dpi.unwrap_or(ctx.image_max_dpi))
  } else {
    None
  };

  let image_node = LayoutNode::Image {
    path: image_path.clone(),
    width,
    height,
    target_dpi,
  };

  let caption_nodes =
    caption.map(|(position, inlines)| return (position, build_caption(ctx, &style.caption, inlines, number, state)));
  let spec = FloatSpec {
    top_margin: style.top_margin,
    bottom_margin: style.bottom_margin,
    inner_margin: style.inner_margin,
  };
  return wrap_float(image_node, caption_nodes, &spec);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{super::test_support, *};
  use crate::config::Style as ReadStyle;

  #[test]
  fn lower_figure_emits_image_and_caption_in_bottom_order() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let caption = [ResolvedInline::Text("せいらん".to_string())];

    // Act
    let nodes = lower_figure(
      &ctx,
      &AssetId::new("./images/seiran.jpg"),
      Some(Length::pt(80.0)),
      Some(Length::pt(60.0)),
      ImageOverrides::default(),
      Some((CaptionPosition::Bottom, &caption)),
      "1",
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })));
    let LayoutNode::VBox { children, .. } = nodes.get(1).expect("VBox があるはず") else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    let LayoutNode::Image {
      path,
      width,
      height,
      target_dpi,
    } = children.first().expect("先頭は画像")
    else {
      panic!("先頭は Image であるべき: {children:?}");
    };
    assert_eq!(path.as_str(), "./images/seiran.jpg");
    assert!((width.expect("width 指定あり").to_pt() - 80.0).abs() < 0.01);
    assert!((height.expect("height 指定あり").to_pt() - 60.0).abs() < 0.01);
    assert_eq!(*target_dpi, Some(300));

    let caption_text = children.iter().find_map(|n| match n {
      LayoutNode::Text(text, _) => return Some(text.as_str()),
      _ => return None,
    });
    assert_eq!(caption_text, Some("Figure 1: せいらん"));
  }

  #[test]
  fn lower_figure_caption_position_top_swaps_order() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let caption = [ResolvedInline::Text("せいらん".to_string())];

    // Act
    let nodes = lower_figure(
      &ctx,
      &AssetId::new("a.png"),
      Some(Length::pt(10.0)),
      Some(Length::pt(10.0)),
      ImageOverrides::default(),
      Some((CaptionPosition::Top, &caption)),
      "2",
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let first_text_idx = children.iter().position(|n| matches!(n, LayoutNode::Text(_, _))).expect("Text あり");
    let first_image_idx = children.iter().position(|n| matches!(n, LayoutNode::Image { .. })).expect("Image あり");
    assert!(first_text_idx < first_image_idx, "Top: caption が image の前");
  }

  #[test]
  fn lower_figure_without_caption_omits_caption_node() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_figure(
      &ctx,
      &AssetId::new("a.png"),
      Some(Length::pt(10.0)),
      Some(Length::pt(10.0)),
      ImageOverrides::default(),
      None,
      "3",
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let has_text = children.iter().any(|n| matches!(n, LayoutNode::Text(_, _)));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
    let has_image = children.iter().any(|n| matches!(n, LayoutNode::Image { .. }));
    assert!(has_image, "画像は出力されている: {children:?}");
  }

  #[test]
  fn lower_figure_per_image_downsample_false_yields_no_target_dpi() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let overrides = ImageOverrides {
      downsample: Some(false),
      ..ImageOverrides::default()
    };
    let nodes = lower_figure(
      &ctx,
      &AssetId::new("a.png"),
      None,
      None,
      overrides,
      None,
      "1",
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let LayoutNode::Image { target_dpi, .. } = children.first().expect("画像") else {
      panic!("Image が期待: {children:?}");
    };
    assert!(target_dpi.is_none());
  }

  #[test]
  fn lower_figure_per_image_dpi_overrides_style() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let overrides = ImageOverrides {
      dpi: Some(600),
      ..ImageOverrides::default()
    };
    let nodes = lower_figure(
      &ctx,
      &AssetId::new("a.png"),
      None,
      None,
      overrides,
      None,
      "1",
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let LayoutNode::Image { target_dpi, .. } = children.first().expect("画像") else {
      panic!("Image が期待: {children:?}");
    };
    assert_eq!(*target_dpi, Some(600));
  }

  #[test]
  fn lower_figure_style_downsample_false_yields_no_target_dpi() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_image_defaults(300, false);

    // Act
    let nodes = lower_figure(
      &ctx,
      &AssetId::new("a.png"),
      None,
      None,
      ImageOverrides::default(),
      None,
      "1",
      &mut LoweringState::new(&test_support::document(&[])),
    );

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let LayoutNode::Image { target_dpi, .. } = children.first().expect("画像") else {
      panic!("Image が期待: {children:?}");
    };
    assert!(target_dpi.is_none());
  }
}
