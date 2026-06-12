//! 図表（フロート）共通のキャプション構築と `VBox` 包み
//!
//! `figure` / `table`（および将来の定理環境などのブロック要素）が共用する。
//! キャプションは [`expand_template`] で展開するため、キャプション内の
//! 書体指定・インライン数式もスタイルを保持したまま埋め込まれる。

use document::{CaptionPosition, InlineNode};
use read_style::CaptionStyle;
use types::{FontKind, Length};

use super::{LoweringContext, LoweringError, template::expand_template};
use crate::layout_node::{LayoutNode, TextStyle};

/// キャプション本体（`format` テンプレの `{number}` / `{title}` を埋めた `LayoutNode` 列）を生成する
///
/// # Errors
///
/// キャプション内に未解決の `\ref` がある場合に [`LoweringError::UnresolvedReference`] を返します。
pub(super) fn build_caption(
  ctx: &LoweringContext,
  caption_style: &CaptionStyle,
  inlines: &[InlineNode],
  number: &str,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let base_style = TextStyle {
    font_size: caption_style.font_size.to_pt(),
    font_kind: FontKind::Serif,
  };
  return expand_template(ctx, &caption_style.format, number, inlines, base_style);
}

/// フロートの余白の指定
// 全フィールドが余白（*_margin）のみの構造体なので postfix 警告は意図どおり
#[allow(clippy::struct_field_names)]
pub(super) struct FloatSpec {
  /// フロート全体の上マージン（VBox の前に Vkern として出力）
  pub top_margin: Length,
  /// フロート全体の下マージン（VBox の `margin_bottom`）
  pub bottom_margin: Length,
  /// 本体とキャプションの間に入れる余白。`None` なら Vkern を出力しない
  pub inner_margin: Option<Length>,
}

/// 本体とキャプションを `caption_position` の順序で積み、上下マージン付きの `VBox` で包む
///
/// `caption` が `None` のときはキャプション行を一切出力しない。
/// 本体とキャプションの縦区切りは `Vkern`（`inner_margin`）のみで表し、
/// ブロック境界の判断は縦組版層（`build_blocks` の `VBox` 再帰平坦化）に委ねる。
pub(super) fn wrap_float(
  main: LayoutNode,
  caption: Option<(CaptionPosition, Vec<LayoutNode>)>,
  spec: &FloatSpec,
) -> Vec<LayoutNode> {
  let mut children = Vec::new();
  match caption {
    Some((CaptionPosition::Top, caption_nodes)) => {
      children.extend(caption_nodes);
      if let Some(margin) = spec.inner_margin {
        children.push(LayoutNode::Vkern { length: margin });
      }
      children.push(main);
    },
    Some((CaptionPosition::Bottom, caption_nodes)) => {
      children.push(main);
      if let Some(margin) = spec.inner_margin {
        children.push(LayoutNode::Vkern { length: margin });
      }
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
    },
  ];
}
