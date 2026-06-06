//! 図環境（`DocNode::Figure`）の lowering
//!
//! 画像 ([`LayoutNode::Image`]) と、`FigureStyle.caption` で書式化したキャプション
//! テキストを [`FigureStyle.caption_position`] の指定順序で `VBox` に詰めます。
//!
//! 画像サイズ（width / height）は mm 入力を pt（72/25.4 倍）に変換します。

use parser::document::InlineNode;
use read_style::CaptionPosition;
use types::{FontKind, Length};

use super::{LoweringContext, LoweringError, inline::inline_nodes_to_plain_text};
use crate::layout_node::{LayoutNode, Style};

/// キャプション文字列を `format` テンプレートで構築する
///
/// - `{number}` は通し番号で置換
/// - `{title}` はキャプション本文で置換（未指定なら空文字）
fn format_caption(template: &str, number: &str, title: &str) -> String {
  return template.replace("{number}", number).replace("{title}", title);
}

/// 図をレイアウトノードに変換する
///
/// 画像とキャプションを `caption_position` の指定に従って積み、上下マージン付きの
/// `VBox` で囲んで返す。キャプションが `None` でもキャプションタイトル部分を
/// 空文字として番号のみ表示する（例: `"Figure 1: "`）。
///
/// # Errors
///
/// キャプション内に未解決の `\ref` がある場合に [`LoweringError::UnresolvedReference`] を返します。
pub(super) fn lower_figure(
  ctx: &LoweringContext,
  image_path: &str,
  width: Length,
  height: Length,
  caption: Option<&[InlineNode]>,
  number: &str,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let style = &ctx.style.core.figure;

  let image_node = LayoutNode::Image {
    path: image_path.to_string(),
    width: width.to_pt(),
    height: height.to_pt(),
  };

  let title_text = match caption {
    Some(inlines) => inline_nodes_to_plain_text(inlines)?,
    None => String::new(),
  };
  let caption_text = format_caption(&style.caption.format, number, &title_text);
  let caption_node = LayoutNode::Text(
    caption_text,
    Style {
      font_size: style.caption.font_size.to_pt(),
      font_kind: FontKind::Serif,
    },
  );

  let inner_gap = LayoutNode::Vkern {
    point: style.inner_margin.to_pt(),
  };

  let mut children = Vec::new();
  match style.caption_position {
    CaptionPosition::Top => {
      children.push(caption_node);
      children.push(LayoutNode::LineBreak);
      children.push(inner_gap);
      children.push(image_node);
      children.push(LayoutNode::LineBreak);
    },
    CaptionPosition::Bottom => {
      children.push(image_node);
      children.push(LayoutNode::LineBreak);
      children.push(inner_gap);
      children.push(caption_node);
      children.push(LayoutNode::LineBreak);
    },
  }

  let result = vec![
    LayoutNode::Vkern {
      point: style.top_margin.to_pt(),
    },
    LayoutNode::VBox {
      children,
      margin_bottom: style.bottom_margin.to_pt(),
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
  fn lower_figure_emits_image_and_caption_in_default_order() {
    // Arrange — デフォルトは caption_position = Bottom
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_figure(
      &ctx,
      "./images/seiran.jpg",
      Length::pt(80.0),
      Length::pt(60.0),
      Some(&[InlineNode::Text("せいらん".to_string())]),
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
    assert!((width - 80.0).abs() < 0.01);
    assert!((height - 60.0).abs() < 0.01);

    let caption_text = children.iter().find_map(|n| match n {
      LayoutNode::Text(text, _) => Some(text.as_str()),
      _ => None,
    });
    assert_eq!(caption_text, Some("Figure 1: せいらん"));
  }

  #[test]
  fn lower_figure_caption_position_top_swaps_order() {
    // Arrange
    let mut style = ReadStyle::default();
    style.core.figure.caption_position = CaptionPosition::Top;
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_figure(&ctx, "a.png", Length::pt(10.0), Length::pt(10.0), None, "2").expect("失敗しない");

    // Assert — VBox 内で キャプションが画像より前
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("VBox が期待");
    };
    let first_text_idx = children.iter().position(|n| matches!(n, LayoutNode::Text(_, _))).expect("Text あり");
    let first_image_idx = children.iter().position(|n| matches!(n, LayoutNode::Image { .. })).expect("Image あり");
    assert!(first_text_idx < first_image_idx, "Top: caption が image の前");
  }
}
