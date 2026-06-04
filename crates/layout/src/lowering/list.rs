//! リスト（`DocNode::List`）の lowering

use parser::document::ListItem;

use super::{LoweringContext, LoweringError, lower_nodes};
use crate::layout_node::{LayoutNode, Style};

/// リストをレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] リストのネスト対応（ネストレベルに応じたインデント量の変更）
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[ListItem],
) -> Result<Vec<LayoutNode>, LoweringError> {
  let list_style = &ctx.style.list;
  let mut result = Vec::new();

  let marker_style = Style {
    font_size: ctx.default_font_size(),
    font_kind: list_style.marker_font_kind,
  };

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成
    // TODO: ネストレベルに応じたマーカーの変更
    let marker = if ordered {
      format!("{} ", list_style.ordered_format.replace("{number}", &(i + 1).to_string()))
    } else {
      format!("{} ", list_style.unordered_marker)
    };

    // インデント + マーカー + 内容
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Kern {
      point: list_style.indent.to_pt(),
    });
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes(ctx, &item.content)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: list_style.item_margin_bottom.to_pt(),
    });
  }

  return Ok(result);
}
