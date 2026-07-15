//! リスト（`DocNode::List`）の lowering

use config::NumberStyle;
use model::ListItem;

use super::{
  LoweringContext, LoweringError, PendingHeading,
  counter::CounterRegistry,
  layout_node::{LayoutNode, TextStyle},
  lower_nodes_inner,
};

/// ネスト段（深さ 1 以上）の unordered マーカー系列（LaTeX itemize 準拠）
///
/// 深さ `d >= 1` のとき `(d - 1) % 3` で引く。en dash → asterisk → middle dot を循環し、
/// 5 段目（深さ 4）は 2 段目（深さ 1）と同じ `–` に戻る。
const NESTED_UNORDERED_MARKERS: [&str; 3] = ["–", "*", "·"];

/// ネスト段（深さ 1 以上）の ordered マーカー系列（番号書式 + 装飾テンプレート）
///
/// 各要素は `(番号書式, "{number}" を含むテンプレート)`。深さ `d >= 1` のとき `(d - 1) % 3` で引き、
/// `(a)`（小文字英字 + 丸括弧）→ `i.`（小文字ローマ数字）→ `A.`（大文字英字）を循環する。
const NESTED_ORDERED_FORMATS: [(NumberStyle, &str); 3] = [
  (NumberStyle::AlphaLower, "({number})"),
  (NumberStyle::RomanLower, "{number}."),
  (NumberStyle::AlphaUpper, "{number}."),
];

/// リストをレイアウトノードに変換する
///
/// マーカーの見た目は `ctx.list_depth`（ネスト深さ、0 = 最上位）に応じて自動的に切り替わる。
/// 最上位は `style.list` の設定（`unordered_marker` / `ordered_marker_format`）をそのまま使い、
/// 1 段以上ネストした段は [`NESTED_UNORDERED_MARKERS`] / [`NESTED_ORDERED_FORMATS`] の固定系列を
/// `(depth - 1) % 3` で循環的に引く。字下げ量（インデント）は深さに依らず `VBox.indent` で表す。
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[ListItem],
  registry: &mut CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let list_style = &ctx.style.list;
  let depth = ctx.list_depth;
  let mut result = Vec::new();

  // 項目内容には本文の段落先頭字下げを波及させない（マーカー直後への字下げを避ける）。
  // 同時にネスト深さを +1 して渡し、item 内容中のネストしたリストが深さ +1 で lower されるようにする。
  let item_ctx = ctx.with_first_line_indent(model::Length::pt(0.0)).with_list_depth(depth + 1);

  let marker_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: list_style.marker_font_kind,
    color: None,
  };

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成。将来 `\item[override]` の個別上書きを入れる場合は、この深さ別の自動選択の
    // 前段で上書き指定を優先する分岐を挿す（上書きが自動マーカーより優先される）。
    let n = (i + 1) as u32;
    let marker_body = if ordered {
      if depth == 0 {
        list_style.ordered_marker_format.replace("{number}", &n.to_string())
      } else {
        let (number_style, template) = NESTED_ORDERED_FORMATS[(depth - 1) % NESTED_ORDERED_FORMATS.len()];
        template.replace("{number}", &number_style.render(n))
      }
    } else if depth == 0 {
      list_style.unordered_marker.clone()
    } else {
      NESTED_UNORDERED_MARKERS[(depth - 1) % NESTED_UNORDERED_MARKERS.len()].to_string()
    };
    let marker = format!("{marker_body} ");

    // マーカー + 内容。左インデントは VBox.indent（ブロック単位）で表し、折り返し行・
    // ネストにも一律適用する。マーカーは先頭行の行頭インラインとして置く。
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes_inner(&item_ctx, &item.content, registry, headings)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: list_style.item_margin_bottom,
      indent: list_style.indent,
      right_indent: model::Length::pt(0.0),
      align: model::Align::Left,
    });
  }

  return Ok(result);
}

#[cfg(test)]
mod tests {
  use config::Style as ReadStyle;
  use model::{DocNode, FontKind, InlineNode, ListItem};

  use super::*;

  /// テキスト 1 段落だけを内容に持つ `ListItem` を作るヘルパ
  fn item_with_text(text: &str) -> ListItem {
    return ListItem::new(vec![DocNode::Paragraph(vec![InlineNode::Text(text.to_string())])]);
  }

  /// テスト用に新規 `CounterRegistry` / 見出し記録バッファを構築して `lower_list` を呼ぶヘルパ
  fn lower_list_default(
    ctx: &LoweringContext,
    ordered: bool,
    items: &[ListItem],
  ) -> Result<Vec<LayoutNode>, LoweringError> {
    let mut registry = CounterRegistry::from_style(ctx.style);
    let mut headings = Vec::new();
    return lower_list(ctx, ordered, items, &mut registry, &mut headings);
  }

  /// `lower_list` が返す各 item（`VBox`）から先頭のマーカー `Text` を取り出す
  ///
  /// インデントは `VBox.indent`（ブロック単位）で表すため、項目の先頭インラインはマーカー。
  fn marker_of(node: &LayoutNode) -> (&str, TextStyle) {
    let LayoutNode::VBox { children, .. } = node else {
      panic!("item は VBox であるべき: {node:?}");
    };
    let LayoutNode::Text(marker, style) = &children[0] else {
      panic!("先頭はマーカー Text であるべき: {children:?}");
    };
    return (marker, *style);
  }

  #[test]
  fn unordered_list_uses_marker_with_trailing_space() {
    // Arrange — 既定 unordered_marker は "•"
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("apple")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items).expect("解決済み内容なので失敗しない");

    // Assert — マーカーは "• "（末尾スペース付き）
    let (marker, _) = marker_of(&nodes[0]);
    assert_eq!(marker, "• ");
  }

  #[test]
  fn ordered_list_numbers_start_at_one_and_increment() {
    // Arrange — 既定 ordered_marker_format は "{number}."
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];

    // Act
    let nodes = lower_list_default(&ctx, true, &items).expect("失敗しない");

    // Assert — 1,2,3 と連番で展開され、末尾スペースが付く
    assert_eq!(nodes.len(), 3);
    let markers: Vec<&str> = nodes.iter().map(|n| marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
  }

  #[test]
  fn item_vbox_uses_indent_margin_and_marker_style() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("x")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items).expect("失敗しない");

    // Assert — VBox.indent=indent（ブロック単位）・先頭に Kern は出さない、
    // margin_bottom=item_margin_bottom、marker_style は規定どおり
    let list_style = &style.list;
    let (_, marker_style) = marker_of(&nodes[0]);
    let LayoutNode::VBox {
      children,
      margin_bottom,
      indent,
      ..
    } = &nodes[0]
    else {
      panic!("item は VBox");
    };
    assert!(!matches!(children[0], LayoutNode::Kern { .. }), "先頭に Kern は出さない: {children:?}");
    assert!((indent.to_pt() - list_style.indent.to_pt()).abs() < f32::EPSILON);
    assert!((margin_bottom.to_pt() - list_style.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
    assert_eq!(marker_style.font_kind, list_style.marker_font_kind);
    assert_eq!(marker_style.font_kind, FontKind::Serif);
    assert_eq!(marker_style.font_size, ctx.default_font_size());
  }

  #[test]
  fn nested_list_item_also_carries_indent() {
    // Arrange — 項目内容にネストしたリストを含める。各段の item VBox に indent が乗ることで、
    // build_blocks 側で外側 indent と累積され、ネスト項目が段ごとに深く字下げされる。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = DocNode::List {
      ordered: false,
      items: vec![ListItem::new(vec![DocNode::Paragraph(vec![
        InlineNode::Text("inner".to_string()),
      ])])],
    };
    let items = [ListItem::new(vec![
      DocNode::Paragraph(vec![InlineNode::Text("outer".to_string())]),
      nested,
    ])];

    // Act
    let nodes = lower_list_default(&ctx, false, &items).expect("失敗しない");

    // Assert — 外側 item VBox.indent == list.indent、内側ネスト item VBox にも同じ indent が乗る
    let indent = style.list.indent.to_pt();
    let LayoutNode::VBox {
      children,
      indent: outer_indent,
      ..
    } = &nodes[0]
    else {
      panic!("外側 item は VBox");
    };
    assert!((outer_indent.to_pt() - indent).abs() < f32::EPSILON);
    let nested_indent = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { indent, .. } => Some(indent.to_pt()),
        _ => None,
      })
      .expect("ネストした item VBox があるはず");
    assert!((nested_indent - indent).abs() < f32::EPSILON, "ネスト item にも indent が乗る");
  }

  #[test]
  fn empty_items_yield_empty_vec() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_list_default(&ctx, true, &[]).expect("失敗しない");

    // Assert
    assert!(nodes.is_empty());
  }

  /// 種別（ordered フラグ）の列から、各段 1 項目のネストしたリストの items を構築する
  ///
  /// `kinds[0]` は最上位リストの ordered フラグ。返り値は最上位リストの `items` で、
  /// 呼び出し側は `lower_list(ctx, kinds[0], &items)` で lower する。各段は本文 1 段落と、
  /// 最深でなければ次段のリスト（ordered フラグは `kinds[1]`）を 1 つ持つ。
  fn build_nested_items(kinds: &[bool]) -> Vec<ListItem> {
    let mut content = vec![DocNode::Paragraph(vec![InlineNode::Text("x".to_string())])];
    if kinds.len() > 1 {
      content.push(DocNode::List {
        ordered: kinds[1],
        items: build_nested_items(&kinds[1..]),
      });
    }
    return vec![ListItem::new(content)];
  }

  /// item `VBox` の子から、ネストしたリストの先頭項目 `VBox` を取り出す
  ///
  /// 段落は `VBox` にならないため、子のうち最初の `VBox` がネストしたリストの item に相当する。
  fn first_nested_vbox(node: &LayoutNode) -> &LayoutNode {
    let LayoutNode::VBox { children, .. } = node else {
      panic!("item は VBox であるべき: {node:?}");
    };
    return children
      .iter()
      .find(|c| matches!(c, LayoutNode::VBox { .. }))
      .expect("ネストしたリストの item VBox があるはず");
  }

  /// ネストしたリストを `depth` 段辿り、各段の先頭項目のマーカー文字列を外→内の順で集める
  fn markers_along_chain(nodes: &[LayoutNode], depth: usize) -> Vec<String> {
    let mut cur = &nodes[0];
    let mut markers = vec![marker_of(cur).0.to_string()];
    for _ in 1..depth {
      cur = first_nested_vbox(cur);
      markers.push(marker_of(cur).0.to_string());
    }
    return markers;
  }

  #[test]
  fn nested_unordered_markers_vary_by_depth() {
    // Arrange — 3 段ネストした itemize（全段 unordered）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false, false, false];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items).expect("失敗しない");

    // Assert — • → – → *（AC#1）
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "– ", "* "]);
  }

  #[test]
  fn nested_ordered_markers_vary_by_depth() {
    // Arrange — 3 段ネストした enumerate（全段 ordered）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [true, true, true];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items).expect("失敗しない");

    // Assert — 1. → (a) → i.（AC#2）
    assert_eq!(markers_along_chain(&nodes, 3), vec!["1. ", "(a) ", "i. "]);
  }

  #[test]
  fn mixed_nesting_advances_each_kind_by_depth() {
    // Arrange — itemize > enumerate > itemize の混在ネスト（AC#4）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false, true, false];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items).expect("失敗しない");

    // Assert — 外 itemize は深さ 0 の •、中 enumerate は深さ 1 の (a)、内 itemize は深さ 2 の *
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "(a) ", "* "]);
  }

  #[test]
  fn unordered_marker_sequence_cycles_after_fourth_level() {
    // Arrange — 5 段ネストした itemize。5 段目（深さ 4）は 2 段目（深さ 1）の – に戻る
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false; 5];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items).expect("失敗しない");

    // Assert — • → – → * → · → –（循環）
    assert_eq!(markers_along_chain(&nodes, 5), vec!["• ", "– ", "* ", "· ", "– "]);
  }
}
