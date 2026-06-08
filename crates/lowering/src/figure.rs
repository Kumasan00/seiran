//! 図環境（`DocNode::Figure`）の lowering
//!
//! 画像 ([`LayoutNode::Image`]) と、`FigureStyle.caption` で書式化したキャプション
//! テキストを caption の位置指定の順序で `VBox` に詰めます。位置はパーサが
//! ソース上の `\image` / `\caption` の出現順から決定します。caption が `None` の
//! ときはキャプション行を一切出力しません。
//!
//! 画像サイズ（width / height）は mm 入力を pt（72/25.4 倍）に変換します。

use parser::document::{CaptionPosition, InlineNode};
use read_style::FigureStyle;
use types::{FontKind, Length};

use super::{LoweringContext, LoweringError, inline::inline_nodes_to_plain_text};
use crate::layout_node::{LayoutNode, TextStyle};

/// キャプション文字列を `format` テンプレートで構築する
///
/// - `{number}` は通し番号で置換
/// - `{title}` はキャプション本文で置換
fn format_caption(template: &str, number: &str, title: &str) -> String {
  return template.replace("{number}", number).replace("{title}", title);
}

/// キャプション本体（`{number}` / `{title}` を埋めた `LayoutNode::Text`）を生成する
fn build_caption_node(style: &FigureStyle, inlines: &[InlineNode], number: &str) -> Result<LayoutNode, LoweringError> {
  let title_text = inline_nodes_to_plain_text(inlines)?;
  let caption_text = format_caption(&style.caption.format, number, &title_text);
  return Ok(LayoutNode::Text(
    caption_text,
    TextStyle {
      font_size: style.caption.font_size.to_pt(),
      font_kind: FontKind::Serif,
    },
  ));
}

/// 図をレイアウトノードに変換する
///
/// 画像とキャプションを指定された [`CaptionPosition`] の順序で積み、上下マージン付きの
/// `VBox` で囲んで返す。`caption` が `None` のときはキャプション行を出力せず、
/// `VBox` には画像のみが入る。
///
/// # Errors
///
/// キャプション内に未解決の `\ref` がある場合に [`LoweringError::UnresolvedReference`] を返します。
pub(super) fn lower_figure(
  ctx: &LoweringContext,
  image_path: &str,
  width: Option<Length>,
  height: Option<Length>,
  caption: Option<(CaptionPosition, &[InlineNode])>,
  number: &str,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let style = &ctx.style.core.figure;

  let image_node = LayoutNode::Image {
    path: image_path.to_string(),
    width,
    height,
  };

  let mut children = Vec::new();
  match caption {
    Some((CaptionPosition::Top, inlines)) => {
      children.push(build_caption_node(style, inlines, number)?);
      children.push(LayoutNode::LineBreak);
      children.push(LayoutNode::Vkern {
        length: style.inner_margin,
      });
      children.push(image_node);
      children.push(LayoutNode::LineBreak);
    },
    Some((CaptionPosition::Bottom, inlines)) => {
      children.push(image_node);
      children.push(LayoutNode::LineBreak);
      children.push(LayoutNode::Vkern {
        length: style.inner_margin,
      });
      children.push(build_caption_node(style, inlines, number)?);
      children.push(LayoutNode::LineBreak);
    },
    None => {
      children.push(image_node);
      children.push(LayoutNode::LineBreak);
    },
  }

  let result = vec![
    LayoutNode::Vkern {
      length: style.top_margin,
    },
    LayoutNode::VBox {
      children,
      margin_bottom: style.bottom_margin,
    },
  ];
  return Ok(result);
}

#[cfg(test)]
mod tests {
  use parser::document::InlineNode;
  use read_style::Style as ReadStyle;

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
      Some((CaptionPosition::Bottom, &caption)),
      "1",
    )
    .expect("解決済みインラインなのでエラーにならない");

    // Assert — top_margin Vkern → VBox（画像 → LineBreak → 内マージン → キャプション → LineBreak）
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })));
    let LayoutNode::VBox { children, .. } = nodes.get(1).expect("VBox があるはず") else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    let LayoutNode::Image {
      path,
      width,
      height,
    } = children.first().expect("先頭は画像")
    else {
      panic!("先頭は Image であるべき: {children:?}");
    };
    assert_eq!(path, "./images/seiran.jpg");
    assert!((width.expect("width 指定あり").to_pt() - 80.0).abs() < 0.01);
    assert!((height.expect("height 指定あり").to_pt() - 60.0).abs() < 0.01);

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
      lower_figure(&ctx, "a.png", Some(Length::pt(10.0)), Some(Length::pt(10.0)), None, "3").expect("失敗しない");

    // Assert — VBox に Text ノードが含まれていない（"Figure 3: " のような空タイトル行を出さない）
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let has_text = children.iter().any(|n| matches!(n, LayoutNode::Text(_, _)));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
    let has_image = children.iter().any(|n| matches!(n, LayoutNode::Image { .. }));
    assert!(has_image, "画像は出力されている: {children:?}");
  }
}
