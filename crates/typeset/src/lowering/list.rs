//! リスト（`DocNode::List`）の lowering

use model::{Length, ListItem};

use super::{
  LoweringContext, LoweringError, PendingHeading,
  counter::CounterRegistry,
  layout_node::{LayoutNode, TextStyle},
  lower_nodes_inner,
};

/// リストをレイアウトノードに変換する
///
/// マーカーの見た目は `ctx.list_depth`（ネスト深さ、0 = 最上位）に応じて自動的に切り替わる。
/// 最上位は `style.list` の設定（`unordered_marker` / `ordered_marker_format`）をそのまま使い、
/// 1 段以上ネストした段は `style.list` の `nested_unordered_markers` / `nested_ordered_formats`
/// （未指定時は既定シーケンス: en dash → asterisk → middle dot / `(a)` → `i.` → `A.`）を
/// `(depth - 1) % 系列の要素数` で循環的に引く。字下げ量（インデント）は深さに依らず `VBox.indent` で表す。
///
/// 項目間の縦アキ（`margin_bottom`）は `\item` 個別の `item.item_gap` > 環境の `item_gap` 引数 >
/// `style.list.item_margin_bottom` の優先順位で決まる。
pub(super) fn lower_list(
  ctx: &LoweringContext,
  ordered: bool,
  items: &[ListItem],
  start: Option<u32>,
  item_gap: Option<Length>,
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
    // マーカーの生成。`\item[marker=...]` による個別上書きがあれば自動生成より優先する。
    // 連番カウンタ n はこの上書きと独立に i から算出するため、上書きされた項目があっても
    // 後続項目の自動番号はズレない。
    let base = start.unwrap_or(1);
    let offset = u32::try_from(i).expect("リスト項目数は u32 に収まる前提");
    let n = base.saturating_add(offset);
    let marker_body = if let Some(marker) = &item.marker {
      marker.clone()
    } else if ordered {
      if depth == 0 {
        list_style.ordered_marker_format.replace("{number}", &n.to_string())
      } else {
        let format = &list_style.nested_ordered_formats[(depth - 1) % list_style.nested_ordered_formats.len()];
        format.format.replace("{number}", &format.number_style.render(n))
      }
    } else if depth == 0 {
      list_style.unordered_marker.clone()
    } else {
      list_style.nested_unordered_markers[(depth - 1) % list_style.nested_unordered_markers.len()].clone()
    };

    // マーカー + 内容。左インデントは VBox.indent（ブロック単位）で表し、折り返し行・
    // ネストにも一律適用する。マーカーは先頭行の行頭インラインとして置く。`marker=""` の
    // 明示指定時（marker_body が空）はマーカー Text 自体を出さず、ぶら下げインデントのみにする。
    let mut item_nodes = Vec::new();
    if !marker_body.is_empty() {
      item_nodes.push(LayoutNode::Text(format!("{marker_body} "), marker_style));
    }

    // アイテム内容を変換
    let content_nodes = lower_nodes_inner(&item_ctx, &item.content, registry, headings)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: item.item_gap.or(item_gap).unwrap_or(list_style.item_margin_bottom),
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
    start: Option<u32>,
  ) -> Result<Vec<LayoutNode>, LoweringError> {
    return lower_list_with_gap(ctx, ordered, items, start, None);
  }

  /// `item_gap`（環境単位の縦アキ上書き）も指定できるテストヘルパ
  fn lower_list_with_gap(
    ctx: &LoweringContext,
    ordered: bool,
    items: &[ListItem],
    start: Option<u32>,
    item_gap: Option<model::Length>,
  ) -> Result<Vec<LayoutNode>, LoweringError> {
    let mut registry = CounterRegistry::from_style(ctx.style);
    let mut headings = Vec::new();
    return lower_list(ctx, ordered, items, start, item_gap, &mut registry, &mut headings);
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
    let nodes = lower_list_default(&ctx, false, &items, None).expect("解決済み内容なので失敗しない");

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
    let nodes = lower_list_default(&ctx, true, &items, None).expect("失敗しない");

    // Assert — 1,2,3 と連番で展開され、末尾スペースが付く
    assert_eq!(nodes.len(), 3);
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "2. ", "3. "]);
  }

  #[test]
  fn ordered_list_with_start_numbers_from_start() {
    // Arrange — `enumerate[start=5]` 相当。既定 ordered_marker_format は "{number}."
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];

    // Act
    let nodes = lower_list_default(&ctx, true, &items, Some(5)).expect("失敗しない");

    // Assert — 5,6,7 と start からの連番で展開される
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["5. ", "6. ", "7. "]);
  }

  #[test]
  fn nested_start_does_not_affect_outer_numbering() {
    // Arrange — 外側 enumerate（start 未指定）の 2 番目の item 内容に
    // 内側 enumerate[start=10] を持たせる（AC: ネストは自リストのみに作用）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = DocNode::List {
      ordered: true,
      items: vec![item_with_text("inner-a"), item_with_text("inner-b")],
      start: Some(10),
      item_gap: None,
    };
    let items = [
      item_with_text("outer-a"),
      ListItem::new(vec![
        DocNode::Paragraph(vec![InlineNode::Text("outer-b".to_string())]),
        nested,
      ]),
    ];

    // Act
    let nodes = lower_list_default(&ctx, true, &items, None).expect("失敗しない");

    // Assert — 外側は 1,2 のまま。内側は 10 番目相当（深さ 1 の書式は AlphaLower なので "(j)"）から始まる
    let outer_markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(outer_markers, vec!["1. ", "2. "]);
    let inner_vbox = first_nested_vbox(&nodes[1]);
    assert_eq!(marker_of(inner_vbox).0, "(j) ");
  }

  #[test]
  fn sibling_list_after_start_list_is_unaffected() {
    // Arrange — 同じ registry を共有する 2 つの独立した最上位 enumerate（AC: 兄弟リストへの波及なし）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let doc_nodes = vec![
      DocNode::List {
        ordered: true,
        items: vec![item_with_text("a"), item_with_text("b")],
        start: Some(5),
        item_gap: None,
      },
      DocNode::List {
        ordered: true,
        items: vec![item_with_text("x")],
        start: None,
        item_gap: None,
      },
    ];

    // Act — `lower_nodes` は全ノードで 1 個の CounterRegistry を共有するため、
    // list 呼び出し間の意図しない状態共有があればここで検出できる
    let nodes = crate::lower_nodes(&ctx, &doc_nodes).expect("失敗しない");

    // Assert — 1 つ目は 5,6。2 つ目（兄弟リスト）は start に影響されず 1 から始まる
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["5. ", "6. ", "1. "]);
  }

  #[test]
  fn item_vbox_uses_indent_margin_and_marker_style() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("x")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None).expect("失敗しない");

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
      start: None,
      item_gap: None,
    };
    let items = [ListItem::new(vec![
      DocNode::Paragraph(vec![InlineNode::Text("outer".to_string())]),
      nested,
    ])];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None).expect("失敗しない");

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
        LayoutNode::VBox { indent, .. } => return Some(indent.to_pt()),
        _ => return None,
      })
      .expect("ネストした item VBox があるはず");
    assert!((nested_indent - indent).abs() < f32::EPSILON, "ネスト item にも indent が乗る");
  }

  #[test]
  fn item_marker_override_replaces_auto_marker() {
    // Arrange — 3 項目中 2 番目だけ marker 上書き（AC: 未指定項目の自動番号はズレない）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut items = [
      item_with_text("a"),
      item_with_text("b"),
      item_with_text("c"),
    ];
    items[1].marker = Some("Q1.".to_string());

    // Act
    let nodes = lower_list_default(&ctx, true, &items, None).expect("失敗しない");

    // Assert — 1・3 番目は自動番号のまま、2 番目だけ上書きマーカー
    let markers: Vec<&str> = nodes.iter().map(|n| return marker_of(n).0).collect();
    assert_eq!(markers, vec!["1. ", "Q1. ", "3. "]);
  }

  #[test]
  fn item_marker_override_empty_string_omits_marker_text() {
    // Arrange — marker="" はマーカー非表示・ぶら下げインデントのみ
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut items = [item_with_text("x")];
    items[0].marker = Some(String::new());

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None).expect("失敗しない");

    // Assert — item VBox の先頭子がマーカーではなく内容そのもの（"x"）から始まる。
    // マーカーが出力される場合は先頭が "x " や "• " 等のマーカー文字列になるはずなので、
    // 先頭 Text が内容と完全一致することがマーカー省略の証拠になる。
    let LayoutNode::VBox { children, .. } = &nodes[0] else {
      panic!("item は VBox であるべき: {:?}", nodes[0]);
    };
    let LayoutNode::Text(text, _) = &children[0] else {
      panic!("先頭は内容の Text であるべき: {children:?}");
    };
    assert_eq!(text, "x", "マーカー Text を挟まず内容の Text から始まるべき");
  }

  #[test]
  fn empty_items_yield_empty_vec() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_list_default(&ctx, true, &[], None).expect("失敗しない");

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
        start: None,
        item_gap: None,
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
    let nodes = lower_list_default(&ctx, kinds[0], &items, None).expect("失敗しない");

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
    let nodes = lower_list_default(&ctx, kinds[0], &items, None).expect("失敗しない");

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
    let nodes = lower_list_default(&ctx, kinds[0], &items, None).expect("失敗しない");

    // Assert — 外 itemize は深さ 0 の •、中 enumerate は深さ 1 の (a)、内 itemize は深さ 2 の *
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "(a) ", "* "]);
  }

  #[test]
  fn item_gap_env_override_applies_to_all_items() {
    // Arrange — 環境単位の item_gap 指定は全項目の margin_bottom に反映される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("a"), item_with_text("b")];

    // Act
    let nodes = lower_list_with_gap(&ctx, false, &items, None, Some(model::Length::mm(3.0))).expect("失敗しない");

    // Assert
    for node in &nodes {
      let LayoutNode::VBox { margin_bottom, .. } = node else {
        panic!("item は VBox であるべき: {node:?}");
      };
      assert!((margin_bottom.to_pt() - model::Length::mm(3.0).to_pt()).abs() < f32::EPSILON);
    }
  }

  #[test]
  fn item_gap_item_override_takes_priority_over_env() {
    // Arrange — `\item[item_gap=...]` は環境単位の item_gap より優先される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut items = [item_with_text("a"), item_with_text("b")];
    items[0].item_gap = Some(model::Length::mm(1.0));

    // Act
    let nodes = lower_list_with_gap(&ctx, false, &items, None, Some(model::Length::mm(3.0))).expect("失敗しない");

    // Assert — 1 項目目は個別上書き、2 項目目は環境の item_gap
    let LayoutNode::VBox {
      margin_bottom: gap0,
      ..
    } = &nodes[0]
    else {
      panic!("item は VBox であるべき");
    };
    let LayoutNode::VBox {
      margin_bottom: gap1,
      ..
    } = &nodes[1]
    else {
      panic!("item は VBox であるべき");
    };
    assert!((gap0.to_pt() - model::Length::mm(1.0).to_pt()).abs() < f32::EPSILON);
    assert!((gap1.to_pt() - model::Length::mm(3.0).to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn item_gap_unspecified_falls_back_to_style_default() {
    // Arrange — 環境・項目どちらも未指定なら style.list.item_margin_bottom を使う（回帰）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let items = [item_with_text("a")];

    // Act
    let nodes = lower_list_default(&ctx, false, &items, None).expect("失敗しない");

    // Assert
    let LayoutNode::VBox { margin_bottom, .. } = &nodes[0] else {
      panic!("item は VBox であるべき");
    };
    assert!((margin_bottom.to_pt() - style.list.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn item_gap_does_not_propagate_to_nested_list() {
    // Arrange — 外側 item_gap 指定はネストしたリストへ非伝播（DocNode::List ごとに独立フィールド）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nested = DocNode::List {
      ordered: false,
      items: vec![item_with_text("inner")],
      start: None,
      item_gap: None,
    };
    let items = [ListItem::new(vec![
      DocNode::Paragraph(vec![InlineNode::Text("outer".to_string())]),
      nested,
    ])];

    // Act — 外側 item_gap = 5mm だが、ネストしたリストの lowering には渡らず None で扱われる
    let nodes = lower_list_with_gap(&ctx, false, &items, None, Some(model::Length::mm(5.0))).expect("失敗しない");

    // Assert — ネスト側 item の margin_bottom は style 既定のまま
    let LayoutNode::VBox { children, .. } = &nodes[0] else {
      panic!("外側 item は VBox であるべき");
    };
    let nested_vbox = children
      .iter()
      .find(|c| matches!(c, LayoutNode::VBox { .. }))
      .expect("ネストしたリストの item VBox があるはず");
    let LayoutNode::VBox { margin_bottom, .. } = nested_vbox else {
      panic!("ネスト item は VBox であるべき");
    };
    assert!((margin_bottom.to_pt() - style.list.item_margin_bottom.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn unordered_marker_sequence_cycles_after_fourth_level() {
    // Arrange — 5 段ネストした itemize。5 段目（深さ 4）は 2 段目（深さ 1）の – に戻る
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let kinds = [false; 5];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None).expect("失敗しない");

    // Assert — • → – → * → · → –（循環）
    assert_eq!(markers_along_chain(&nodes, 5), vec!["• ", "– ", "* ", "· ", "– "]);
  }

  #[test]
  fn nested_unordered_markers_use_style_override() {
    // Arrange — style.list.nested_unordered_markers をカスタム系列に上書き
    let mut style = ReadStyle::default();
    style.list.nested_unordered_markers = vec!["§".to_string(), "†".to_string()];
    let ctx = LoweringContext::new(&style);
    let kinds = [false, false, false];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None).expect("失敗しない");

    // Assert — 最上位は既定の • のまま、ネスト段はカスタム系列を (depth - 1) % 2 で循環
    assert_eq!(markers_along_chain(&nodes, 3), vec!["• ", "§ ", "† "]);
  }

  #[test]
  fn nested_ordered_formats_use_style_override() {
    // Arrange — style.list.nested_ordered_formats をカスタム系列に上書き（要素数 2 で循環を確認）
    let mut style = ReadStyle::default();
    style.list.nested_ordered_formats = vec![
      config::NestedOrderedFormat {
        number_style: config::NumberStyle::RomanUpper,
        format: "[{number}]".to_string(),
      },
      config::NestedOrderedFormat {
        number_style: config::NumberStyle::Kanji,
        format: "{number}、".to_string(),
      },
    ];
    let ctx = LoweringContext::new(&style);
    let kinds = [true, true, true];
    let items = build_nested_items(&kinds);

    // Act
    let nodes = lower_list_default(&ctx, kinds[0], &items, None).expect("失敗しない");

    // Assert — 最上位は既定の "1. " のまま、深さ 1 は [I]、深さ 2 は要素数 2 で循環して一、に戻る
    assert_eq!(markers_along_chain(&nodes, 3), vec!["1. ", "[I] ", "一、 "]);
  }
}
