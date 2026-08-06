//! 図環境（`model::HirNodeKind::Figure`）の lowering

use super::{
  LoweringContext, LoweringState,
  float::{FloatSpec, build_caption, wrap_float},
  layout_node::LayoutNode,
};
use crate::{
  length::Length,
  model::{CaptionPosition, HirInline},
  project::ProjectPath,
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
  image_path: &ProjectPath,
  width: Option<Length>,
  height: Option<Length>,
  overrides: ImageOverrides,
  caption: Option<(CaptionPosition, &[HirInline])>,
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
  use super::{
    super::{DocumentContent, lower_sources_with_headings, test_support::analyzed},
    *,
  };
  use crate::{citation::GeneratedCitations, config::Style as ReadStyle};

  /// `.sei` ソースを与えられた文脈で lower するテストヘルパ
  ///
  /// 画像の既定値（`with_image_defaults`）を差し替えるテストがあるため、`LoweringContext` を
  /// 呼び出し側から渡せる形にしてある。
  fn lower_source(ctx: &LoweringContext, source: &str) -> Vec<LayoutNode> {
    let analyzed = analyzed(source);
    let citations = GeneratedCitations::default();
    let (layout, _headings) = lower_sources_with_headings(
      ctx,
      DocumentContent {
        analyzed: &analyzed,
        citations: &citations,
      },
    );
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
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_source(
      &ctx,
      "\\chapter{C}\n\n\\begin{figure}\n\\image[width=80mm, height=60mm]{./images/seiran.jpg}\n\\caption{せいらん}\n\\end{figure}\n",
    );

    // Assert — フロート本体の直前には top_margin の Vkern が入る
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
      LayoutNode::Text(text, _) => return Some(text.as_str()),
      _ => return None,
    });
    assert_eq!(caption_text, Some("Figure 1.1: せいらん"));
  }

  #[test]
  fn lower_figure_caption_position_top_swaps_order() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act — `\caption` を `\image` より前に置くとキャプションは図の上になる
    let nodes = lower_source(
      &ctx,
      "\\begin{figure}\n\\caption{せいらん}\n\\image[width=10mm, height=10mm]{a.png}\n\\end{figure}\n",
    );

    // Assert
    let children = figure_children(&nodes);
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
    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[width=10mm, height=10mm]{a.png}\n\\end{figure}\n");

    // Assert
    let children = figure_children(&nodes);
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
    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[downsample=false]{a.png}\n\\end{figure}\n");

    // Assert
    let LayoutNode::Image { target_dpi, .. } = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }

  #[test]
  fn lower_figure_per_image_dpi_overrides_style() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[dpi=600]{a.png}\n\\end{figure}\n");

    // Assert
    let LayoutNode::Image { target_dpi, .. } = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert_eq!(*target_dpi, Some(600));
  }

  #[test]
  fn lower_figure_style_downsample_false_yields_no_target_dpi() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_image_defaults(300, false);

    // Act
    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image{a.png}\n\\end{figure}\n");

    // Assert
    let LayoutNode::Image { target_dpi, .. } = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }
}
