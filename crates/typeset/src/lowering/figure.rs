//! 図環境（`DocNode::Figure`）の lowering
//!
//! 画像 ([`LayoutNode::Image`]) と、`FigureStyle.caption` で書式化したキャプションを
//! caption の位置指定の順序で `VBox` に詰めます。位置はパーサがソース上の
//! `\image` / `\caption` の出現順から決定します。caption が `None` のときは
//! キャプション行を一切出力しません。キャプション構築と `VBox` 包みは
//! [`super::float`] の共通ヘルパで行います。

use model::{CaptionPosition, InlineNode, Length};

use super::{
  LoweringContext, LoweringError,
  float::{FloatSpec, build_caption, wrap_float},
  layout_node::LayoutNode,
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
///
/// 画像とキャプションを指定された [`CaptionPosition`] の順序で積み、上下マージン付きの
/// `VBox` で囲んで返す。`caption` が `None` のときはキャプション行を出力せず、
/// `VBox` には画像のみが入る。
///
/// per-image の `dpi` / `downsample` 上書きと config `[image].max_dpi` / `[image].downsample`
/// （`ctx.image_max_dpi` / `ctx.image_downsample`）の既定値を解決し、ラスタ画像のダウンサンプリング上限 DPI を `target_dpi` として
/// [`LayoutNode::Image`] に焼き付ける。`downsample` が `false` に解決された場合は `None`
/// （リサイズなし）になる。
///
/// # Errors
///
/// キャプション内に未解決の `\ref` がある場合に [`LoweringError::UnresolvedReference`] を返します。
pub(super) fn lower_figure(
  ctx: &LoweringContext,
  image_path: &str,
  width: Option<Length>,
  height: Option<Length>,
  overrides: ImageOverrides,
  caption: Option<(CaptionPosition, &[InlineNode])>,
  number: &str,
) -> Result<Vec<LayoutNode>, LoweringError> {
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
    path: image_path.to_string(),
    width,
    height,
    target_dpi,
  };

  let caption_nodes = match caption {
    Some((position, inlines)) => Some((position, build_caption(ctx, &style.caption, inlines, number)?)),
    None => None,
  };
  let spec = FloatSpec {
    top_margin: style.top_margin,
    bottom_margin: style.bottom_margin,
    inner_margin: style.inner_margin,
  };
  return Ok(wrap_float(image_node, caption_nodes, &spec));
}

#[cfg(test)]
mod tests {
  use config::Style as ReadStyle;
  use model::InlineNode;

  use super::*;

  #[test]
  fn lower_figure_emits_image_and_caption_in_bottom_order() {
    // Arrange — \image が先（DocNode から CaptionPosition::Bottom が渡される）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let caption = [InlineNode::Text("せいらん".to_string())];

    // Act
    let nodes = lower_figure(
      &ctx,
      "./images/seiran.jpg",
      Some(Length::pt(80.0)),
      Some(Length::pt(60.0)),
      ImageOverrides::default(),
      Some((CaptionPosition::Bottom, &caption)),
      "1",
    )
    .expect("解決済みインラインなのでエラーにならない");

    // Assert — top_margin Vkern → VBox（画像 → 内マージン Vkern → キャプション）
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
    assert_eq!(path, "./images/seiran.jpg");
    assert!((width.expect("width 指定あり").to_pt() - 80.0).abs() < 0.01);
    assert!((height.expect("height 指定あり").to_pt() - 60.0).abs() < 0.01);
    // 既定（config `[image]`）では downsample=true / max_dpi=300 なので target_dpi=Some(300)
    assert_eq!(*target_dpi, Some(300));

    let caption_text = children.iter().find_map(|n| match n {
      LayoutNode::Text(text, _) => Some(text.as_str()),
      _ => None,
    });
    assert_eq!(caption_text, Some("Figure 1: せいらん"));
  }

  #[test]
  fn lower_figure_caption_position_top_swaps_order() {
    // Arrange — DocNode 側で CaptionPosition::Top を指定（\caption が先）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let caption = [InlineNode::Text("せいらん".to_string())];

    // Act
    let nodes = lower_figure(
      &ctx,
      "a.png",
      Some(Length::pt(10.0)),
      Some(Length::pt(10.0)),
      ImageOverrides::default(),
      Some((CaptionPosition::Top, &caption)),
      "2",
    )
    .expect("失敗しない");

    // Assert — VBox 内で キャプションが画像より前
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let first_text_idx = children.iter().position(|n| matches!(n, LayoutNode::Text(_, _))).expect("Text あり");
    let first_image_idx = children.iter().position(|n| matches!(n, LayoutNode::Image { .. })).expect("Image あり");
    assert!(first_text_idx < first_image_idx, "Top: caption が image の前");
  }

  #[test]
  fn lower_figure_without_caption_omits_caption_node() {
    // Arrange — caption が None ならキャプション行は出力しない
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes =
      lower_figure(&ctx, "a.png", Some(Length::pt(10.0)), Some(Length::pt(10.0)), ImageOverrides::default(), None, "3")
        .expect("失敗しない");

    // Assert — VBox に Text ノードが含まれていない（"Figure 3: " のような空タイトル行を出さない）
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
    // Arrange — per-image downsample=false なら target_dpi は None
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let overrides = ImageOverrides {
      downsample: Some(false),
      ..ImageOverrides::default()
    };
    let nodes = lower_figure(&ctx, "a.png", None, None, overrides, None, "1").expect("失敗しない");

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
    // Arrange — per-image dpi=600 は既定（config `[image]` の 300）を上書きする
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let overrides = ImageOverrides {
      dpi: Some(600),
      ..ImageOverrides::default()
    };
    let nodes = lower_figure(&ctx, "a.png", None, None, overrides, None, "1").expect("失敗しない");

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
    // Arrange — グローバル downsample=false（config `[image]` 由来）ならリサイズしない
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style).with_image_defaults(300, false);

    // Act
    let nodes = lower_figure(&ctx, "a.png", None, None, ImageOverrides::default(), None, "1").expect("失敗しない");

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
