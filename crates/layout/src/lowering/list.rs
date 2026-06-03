//! リスト（`DocNode::List`）の lowering

use parser::document::ListItem;
use types::FontKind;

use super::{LoweringContext, LoweringError, lower_nodes};
use crate::layout_node::{LayoutNode, Style};

/// リストをレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] リストのネスト対応（ネストレベルに応じたインデント量の変更）
/// - [ ] 順序付きリスト（enumerate）のマーカー生成（1., 2., 3. 等）
/// - [ ] 順序なしリストのマーカー生成（•, -, ▪ 等、ネストレベルで変更）
/// - [ ] マーカーのフォント・サイズをスタイル設定で制御する
/// - [ ] リスト前後のスペースをスタイル設定で制御する
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[ListItem],
) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut result = Vec::new();
  let indent = 20.0; // TODO: スタイル設定から取得する

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成
    // TODO: ネストレベルに応じたマーカーの変更
    let marker = if ordered {
      format!("{}. ", i + 1)
    } else {
      "• ".to_string()
    };

    let marker_style = Style {
      font_size: ctx.default_font_size(),
      font_kind: FontKind::Serif,
    };

    // インデント + マーカー + 内容
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Kern { point: indent });
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes(ctx, &item.content)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: 4.0, // TODO: スタイル設定から取得する
    });
  }

  return Ok(result);
}
