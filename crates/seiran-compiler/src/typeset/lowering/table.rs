//! 表環境（`document::HirNodeKind::Table`）の lowering

use crate::{
  color::Color,
  document::{ColumnAlign, HirInline, HirInlineKind, HirTable, HirTableRow, NodeId},
  typeset::{
    error::ElementOverflow,
    lowering::{
      LoweringContext, LoweringState,
      float::{FloatCaption, FloatMargins, lower_numbered_float},
      inline::{colored, lower_inlines, quantity_node},
      layout_node::{
        LayoutNode, TableCellContent, TableCellLayout, TableColumn, TableLayout, TableRowLayout, TextStyle,
      },
      quantity::{split_number_pieces, split_quantity_pieces},
    },
  },
};

/// `HirTableRow` の列を [`TableRowLayout`] の列に変換する
///
/// `value_columns` は本体行の列の揃え（`None` は `\head` の行で、列の値を持たない）。`Decimal` 列で始まり
/// `column_span == 1` のセルが列の値のセルで、[`lower_decimal_cell`] で左右に分ける。
fn lower_rows(
  ctx: &LoweringContext<'_>,
  rows: &[HirTableRow],
  value_columns: Option<&[ColumnAlign]>,
  cell_style: TextStyle,
  state: &mut LoweringState<'_>,
) -> Result<Vec<TableRowLayout>, ElementOverflow> {
  let mut result = Vec::with_capacity(rows.len());
  for row in rows {
    let mut cells = Vec::with_capacity(row.cells.len());
    let mut column = 0usize;
    for cell in &row.cells {
      let is_value = cell.column_span == 1
        && value_columns.is_some_and(|columns| return columns.get(column) == Some(&ColumnAlign::Decimal));
      column += cell.column_span as usize;
      let content = if is_value {
        lower_decimal_cell(ctx, &cell.content, cell_style, state)?
      } else {
        TableCellContent::Inline(lower_inlines(ctx, &cell.content, cell_style, state)?)
      };
      cells.push(TableCellLayout {
        content,
        column_span: cell.column_span,
        row_span: cell.row_span,
        rule_below: cell.rule_below,
      });
    }
    result.push(TableRowLayout {
      origin: row.id,
      cells,
      separator: row.separator,
    });
  }
  return Ok(result);
}

/// decimal 列の値のセルを、符号 + 整数部と、その後ろ（数値の残り・単位・脚注マーク・索引語）に分ける
///
/// 空のセルは分ける値が無いので空の [`TableCellContent::Inline`]。
fn lower_decimal_cell(
  ctx: &LoweringContext<'_>,
  content: &[HirInline],
  cell_style: TextStyle,
  state: &mut LoweringState<'_>,
) -> Result<TableCellContent, ElementOverflow> {
  let Some((head, trailing)) = content.split_first() else {
    return Ok(TableCellContent::Inline(Vec::new()));
  };
  let (inner, style) = match &head.kind {
    HirInlineKind::Colored { color, children } => (children.as_slice(), colored(cell_style, *color)),
    _ => (std::slice::from_ref(head), cell_style),
  };
  let (left, right) = match inner {
    [
      HirInline {
        kind: HirInlineKind::Number(numeral),
        ..
      },
    ] => split_number_pieces(numeral, &ctx.style.quantity),
    [
      HirInline {
        kind: HirInlineKind::Quantity { numeral, unit },
        ..
      },
    ] => split_quantity_pieces(numeral, unit, &ctx.style.quantity),
    _ => unreachable!("decimal 列の値のセルの中身は frontend（TableDecimalCell）が数値要素の形に限る"),
  };
  let integer = quantity_node(ctx, left, style);
  let mut rest = Vec::new();
  if !right.is_empty() {
    rest.push(quantity_node(ctx, right, style));
  }
  rest.extend(lower_inlines(ctx, trailing, cell_style, state)?);
  return Ok(TableCellContent::Decimal { integer, rest });
}

/// 表をレイアウトノードに変換する
pub(super) fn lower_table(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  table: &HirTable,
  state: &mut LoweringState<'_>,
) -> Result<Vec<LayoutNode>, ElementOverflow> {
  let style = &ctx.style.table;

  let body_style = TextStyle {
    font_size: ctx.default_font_size(),
    typeface: ctx.body_typeface,
    color: None,
    script_level: None,
    math_operator: None,
  };
  let head_style = TextStyle {
    font_size: body_style.font_size,
    typeface: style.head_typeface,
    color: None,
    script_level: None,
    math_operator: None,
  };

  let margins = FloatMargins {
    top: style.top_margin,
    bottom: style.bottom_margin,
    inner: style.inner_margin,
  };
  let caption = FloatCaption {
    style: &style.caption,
    inlines: table.caption.as_deref(),
    position: table.caption_position,
  };
  return lower_numbered_float(ctx, id, caption, &margins, state, |state| {
    return Ok(vec![LayoutNode::Table(TableLayout {
      origin: id,
      columns: table
        .columns
        .iter()
        .zip(&table.widths)
        .map(|(align, width)| {
          return TableColumn {
            align: *align,
            width: *width,
          };
        })
        .collect(),
      head: lower_rows(ctx, &table.head, None, head_style, state)?,
      rows: lower_rows(ctx, &table.rows, Some(&table.columns), body_style, state)?,
      breakable: table.breakable,
      cell_padding: style.cell_padding,
      outer_rule_thickness: style.outer_rule_thickness,
      rule_thickness: style.rule_thickness,
      rule_gap: style.rule_gap,
      row_gap: style.row_gap,
      partial_rule_thickness: style.partial_rule_thickness,
      partial_rule_trim: style.partial_rule_trim,
      rule_color: style.rule_color.map(Color::rgb),
    })]);
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    document::{ColumnAlign, ColumnWidth, RowSeparator, Typeface},
    length::Length,
    style::Style as ReadStyle,
    typeset::lowering::{
      layout_node::{InlineNode, QuantityPiece},
      test_support::{analyzed, inlines, lower, paragraph_layout},
    },
  };

  /// `.sei` ソースを lower してレイアウトノード列を返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> { return lower(style, &analyzed(source)); }

  /// 表の本体 `VBox` の子要素列を `VBox` の入れ子を辿って探すヘルパ
  ///
  /// 引用は本体を 1 枚の `VBox` で包むため、その中の表は最上位から 1 段深い位置に現れる。
  fn find_table_children(nodes: &[LayoutNode]) -> Option<&[LayoutNode]> {
    for node in nodes {
      let LayoutNode::VBox { children, .. } = node else {
        continue;
      };
      if children.iter().any(|c| matches!(c, LayoutNode::Table(_))) {
        return Some(children.as_slice());
      }
      if let Some(found) = find_table_children(children) {
        return Some(found);
      }
    }
    return None;
  }

  /// 表の本体 `VBox` の子要素列を取り出すヘルパ
  fn table_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    return find_table_children(nodes).expect("表本体の VBox があるはず");
  }

  /// 分けないセルのインライン列を取り出すヘルパ
  fn inline_content(cell: &TableCellLayout) -> &[InlineNode] {
    let TableCellContent::Inline(nodes) = &cell.content else {
      panic!("分けないセルのはず: {cell:?}");
    };
    return nodes;
  }

  /// `lower_table` の結果から `TableLayout` を取り出すヘルパ
  fn find_table(nodes: &[LayoutNode]) -> &TableLayout {
    return table_children(nodes)
      .iter()
      .find_map(|n| match n {
        LayoutNode::Table(t) => return Some(t),
        _ => return None,
      })
      .expect("VBox 内に Table があるはず");
  }

  #[test]
  fn lower_table_builds_columns_and_rows() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{table}[columns=left right]\n\\head{\n\\row{Name & Score}\n}\n\
       \\row{Alice & 92}\n\\row{Bob & 88}\n\\end{table}\n",
    );

    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })));
    let table = find_table(&nodes);
    assert_eq!(table.columns.len(), 2);
    assert_eq!(table.columns[0].align, ColumnAlign::Left);
    assert_eq!(table.columns[1].align, ColumnAlign::Right);
    assert_eq!(table.head.len(), 1);
    assert_eq!(table.rows.len(), 2);
    assert!(table.breakable);
  }

  #[test]
  fn lower_table_head_cells_follow_style_head_typeface() {
    let mut style = ReadStyle::default();
    style.table.head_typeface = Typeface::SansSerif;

    let nodes = lower_source(&style, "\\begin{table}\n\\head{\n\\row{Name}\n}\n\\row{Alice}\n\\end{table}\n");

    let table = find_table(&nodes);
    let InlineNode::Text(_, head_style) = inline_content(&table.head[0].cells[0])[0] else {
      panic!("ヘッダセルは Text であるべき");
    };
    assert_eq!(head_style.typeface, Typeface::SansSerif);
    let InlineNode::Text(_, body_style) = inline_content(&table.rows[0].cells[0])[0] else {
      panic!("本体セルは Text であるべき");
    };
    assert_eq!(body_style.typeface, Typeface::Serif, "本体セルは文脈の本文書体（最上位なので [text]）のまま");
  }

  #[test]
  fn lower_table_in_theorem_body_cells_use_theorem_typeface() {
    // 既定 style で [theorems.theorem].typeface は serif_italic
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{theorem}\n定理本体の段落。\n\n\\begin{table}\n\\head{\n\\row{Name}\n}\n\\row{Alice}\n\
       \\end{table}\n\\end{theorem}\n",
    );

    let table = find_table(&nodes);
    let InlineNode::Text(_, body_style) = inline_content(&table.rows[0].cells[0])[0] else {
      panic!("本体セルは Text であるべき");
    };
    assert_eq!(body_style.typeface, Typeface::SerifItalic, "本体セルは定理本体の書体に従う");
    let InlineNode::Text(_, head_style) = inline_content(&table.head[0].cells[0])[0] else {
      panic!("ヘッダセルは Text であるべき");
    };
    assert_eq!(
      head_style.typeface,
      Typeface::SerifBold,
      "ヘッダ行は [table].head_typeface のままで、スコープ本文書体の影響を受けない"
    );
  }

  #[test]
  fn lower_table_in_quote_body_cells_use_quote_typeface() {
    let mut style = ReadStyle::default();
    style.quote.typeface = Typeface::SansSerif;

    let nodes = lower_source(&style, "\\begin{quote}\n\\begin{table}\n\\row{Alice}\n\\end{table}\n\\end{quote}\n");

    let table = find_table(&nodes);
    let InlineNode::Text(_, body_style) = inline_content(&table.rows[0].cells[0])[0] else {
      panic!("本体セルは Text であるべき");
    };
    assert_eq!(body_style.typeface, Typeface::SansSerif, "本体セルは引用の書体に従う");
  }

  #[test]
  fn lower_table_caption_bottom_places_caption_after_table() {
    let style = ReadStyle::default();

    // `\caption` を行より後ろに置くとキャプションは表の下になる
    let nodes = lower_source(&style, "\\chapter{C}\n\n\\begin{table}\n\\row{A}\n\\caption{得点表}\n\\end{table}\n");

    let children = table_children(&nodes);
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let caption_idx =
      children.iter().position(|n| matches!(n, LayoutNode::Paragraph(_))).expect("キャプション段落あり");
    assert!(matches!(
      &paragraph_layout(&children[caption_idx]).content[0],
      InlineNode::Text(text, _) if text == "Table 1.1: 得点表"
    ));
    assert!(table_idx < caption_idx, "Bottom: table がキャプションの前");
    assert!(
      matches!(children.get(table_idx + 1), Some(LayoutNode::Vkern { length }) if (length.to_pt() - style.table.inner_margin.to_pt()).abs() < f32::EPSILON),
      "本体の直後に inner_margin の Vkern が入る: {children:?}"
    );
  }

  #[test]
  fn lower_table_caption_top_places_caption_before_table() {
    let style = ReadStyle::default();

    // `\caption` を行より前に置くとキャプションは表の上になる
    let nodes = lower_source(&style, "\\chapter{C}\n\n\\begin{table}\n\\caption{得点表}\n\\row{A}\n\\end{table}\n");

    let children = table_children(&nodes);
    let table_idx = children.iter().position(|n| matches!(n, LayoutNode::Table(_))).expect("Table あり");
    let caption_idx =
      children.iter().position(|n| matches!(n, LayoutNode::Paragraph(_))).expect("キャプション段落あり");
    assert!(matches!(
      &paragraph_layout(&children[caption_idx]).content[0],
      InlineNode::Text(text, _) if text.starts_with("Table 1.1")
    ));
    assert!(caption_idx < table_idx, "Top: キャプションが table の前");
  }

  #[test]
  fn lower_table_caption_follows_style_caption_typeface() {
    // 図とは別の書体を立て、表キャプションが `[table.caption]` の側だけを見ることを確かめる
    let mut style = ReadStyle::default();
    style.table.caption.typeface = Typeface::Monospace;
    style.figure.caption.typeface = Typeface::SansSerif;

    let nodes = lower_source(&style, "\\chapter{C}\n\n\\begin{table}\n\\row{A}\n\\caption{得点表}\n\\end{table}\n");

    let caption = inlines(table_children(&nodes))
      .find_map(|n| match n {
        InlineNode::Text(text, text_style) if text.starts_with("Table 1.1") => {
          return Some(*text_style);
        },
        _ => return None,
      })
      .expect("キャプション Text あり");
    assert_eq!(caption.typeface, Typeface::Monospace);
  }

  #[test]
  fn lower_table_preserves_column_widths() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{table}[columns=left center right, widths=40mm 0.25 *]\n\\row{a & b & c}\n\\end{table}\n",
    );

    let table = find_table(&nodes);
    assert_eq!(table.columns.len(), 3);
    assert!(
      matches!(table.columns[0].width, ColumnWidth::Fixed(l) if (l.to_pt() - Length::mm(40.0).to_pt()).abs() < f32::EPSILON)
    );
    assert!(matches!(table.columns[1].width, ColumnWidth::Ratio(r) if (r - 0.25).abs() < f32::EPSILON));
    assert!(matches!(table.columns[2].width, ColumnWidth::Flex));
    assert_eq!(table.columns[1].align, ColumnAlign::Center);
  }

  #[test]
  fn lower_table_resolves_padding_and_rules_from_style() {
    let mut style = ReadStyle::default();
    style.table.cell_padding = Length::pt(3.0);
    style.table.outer_rule_thickness = Length::pt(2.5);
    style.table.rule_thickness = Length::pt(1.5);
    style.table.rule_gap = Length::pt(3.5);
    style.table.row_gap = Length::pt(6.5);
    style.table.partial_rule_thickness = Length::pt(0.7);
    style.table.partial_rule_trim = Length::pt(6.0);
    style.table.rule_color = Some("#090909".parse().expect("16 進の色は読めるはず"));

    let nodes = lower_source(&style, "\\begin{table}\n\\row{A}\n\\end{table}\n");

    let table = find_table(&nodes);
    assert_eq!(table.cell_padding, Length::pt(3.0));
    assert_eq!(table.outer_rule_thickness, Length::pt(2.5));
    assert_eq!(table.rule_thickness, Length::pt(1.5));
    assert_eq!(table.rule_gap, Length::pt(3.5));
    assert_eq!(table.row_gap, Length::pt(6.5));
    assert_eq!(table.partial_rule_thickness, Length::pt(0.7));
    assert_eq!(table.partial_rule_trim, Length::pt(6.0));
    assert_eq!(table.rule_color, Some([9, 9, 9]));
  }

  #[test]
  fn lower_table_breakable_false_is_preserved() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{table}[breakable=false]\n\\row{A}\n\\end{table}\n");

    let table = find_table(&nodes);
    assert!(!table.breakable);
  }

  #[test]
  fn lower_table_preserves_the_row_separator() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{table}\n\\row{A}\n\\row[separator=gap]{B}\n\\end{table}\n");

    let table = find_table(&nodes);
    assert_eq!(table.rows[0].separator, None);
    assert_eq!(table.rows[1].separator, Some(RowSeparator::Gap));
  }

  #[test]
  fn lower_table_preserves_the_cell_rule_below() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{table}\n\\row{\\cell[rule_below]{A} & B}\n\\row{C & D}\n\\end{table}\n");

    let table = find_table(&nodes);
    let flags: Vec<bool> = table.rows[0].cells.iter().map(|cell| return cell.rule_below).collect();
    assert_eq!(flags, vec![true, false]);
  }

  #[test]
  fn lower_table_preserves_the_cell_row_span() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{table}\n\\row{\\cell[rowspan=2]{A} & B}\n\\row{ & C}\n\\end{table}\n");

    let table = find_table(&nodes);
    let spans: Vec<u32> = table.rows[0].cells.iter().map(|cell| return cell.row_span).collect();
    assert_eq!(spans, vec![2, 1]);
  }

  #[test]
  fn lower_table_without_caption_omits_caption_text() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, "\\begin{table}\n\\row{A}\n\\end{table}\n");

    let children = table_children(&nodes);
    let has_text = inlines(children).any(|n| matches!(n, InlineNode::Text(_, _)));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
  }

  #[test]
  fn lower_table_cell_footnote_shares_document_wide_counter() {
    let style = ReadStyle::default();

    let nodes =
      lower_source(&style, "本文\\footnote{body note}\n\n\\begin{table}\n\\row{\\footnote{cell note}}\n\\end{table}\n");

    let table = find_table(&nodes);
    assert!(
      matches!(inline_content(&table.rows[0].cells[0])[1], InlineNode::Footnote { number: 2, .. }),
      "{:?}",
      inline_content(&table.rows[0].cells[0])
    );
  }

  #[test]
  fn lower_table_numbers_head_cell_footnote_before_body_cell_footnote() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{table}\n\\head{\\row{H\\footnote{head note}}}\n\\row{A\\footnote{cell note}}\n\\end{table}\n",
    );

    let table = find_table(&nodes);
    let number_in = |content: &[InlineNode]| {
      return content.iter().find_map(|n| match n {
        InlineNode::Footnote { number, .. } => return Some(*number),
        _ => return None,
      });
    };
    assert_eq!(number_in(inline_content(&table.head[0].cells[0])), Some(1), "ヘッダのセルが 1 番: {table:?}");
    assert_eq!(number_in(inline_content(&table.rows[0].cells[0])), Some(2), "本文セルが 2 番: {table:?}");
  }

  #[test]
  fn lower_table_numbers_cell_footnote_before_caption_footnote() {
    let style = ReadStyle::default();

    let nodes = lower_source(
      &style,
      "\\begin{table}\n\\row{A\\footnote{cell note}}\n\\caption{C\\footnote{caption note}}\n\\end{table}\n",
    );

    let table = find_table(&nodes);
    let cell_number = inline_content(&table.rows[0].cells[0]).iter().find_map(|n| match n {
      InlineNode::Footnote { number, .. } => return Some(*number),
      _ => return None,
    });
    assert_eq!(cell_number, Some(1), "セルの脚注が 1 番: {:?}", inline_content(&table.rows[0].cells[0]));
    let caption_numbers: Vec<u32> = inlines(table_children(&nodes))
      .filter_map(|n| match n {
        InlineNode::Footnote { number, .. } => return Some(*number),
        _ => return None,
      })
      .collect();
    assert_eq!(caption_numbers, vec![2], "キャプションの脚注が 2 番: {nodes:?}");
  }

  #[test]
  fn lower_table_splits_decimal_value_cells() {
    let nodes = lower_source(
      &ReadStyle::default(),
      "\\begin{table}[columns=left decimal]\n\\head{\\row{名前 & 値}}\n\\row{a & \\num{12.5}}\n\\row{b & }\n\\end{table}\n",
    );
    let table = find_table(&nodes);

    assert!(matches!(&table.head[0].cells[1].content, TableCellContent::Inline(_)), "見出しは分けない");
    let TableCellContent::Decimal { integer, rest } = &table.rows[0].cells[1].content else {
      panic!("値のセルは分ける: {table:?}");
    };
    assert!(
      matches!(integer, InlineNode::Quantity { pieces, .. } if pieces == &[QuantityPiece::Text("12".to_string())])
    );
    assert!(
      matches!(rest.as_slice(), [InlineNode::Quantity { pieces, .. }] if pieces == &[QuantityPiece::Text(".5".to_string())]),
      "{rest:?}"
    );
    assert!(
      matches!(&table.rows[1].cells[1].content, TableCellContent::Inline(nodes) if nodes.is_empty()),
      "空の値は空の Inline"
    );
    assert!(matches!(&table.rows[0].cells[0].content, TableCellContent::Inline(_)), "left 列は分けない");
  }

  #[test]
  fn lower_table_decimal_cell_keeps_marks_after_the_rest() {
    let nodes = lower_source(
      &ReadStyle::default(),
      "\\begin{table}[columns=decimal]\n\\row{\\color[color=#cc0000]{\\num{7}}\\footnote{注}}\n\\end{table}\n",
    );
    let table = find_table(&nodes);

    let TableCellContent::Decimal { integer, rest } = &table.rows[0].cells[0].content else {
      panic!("値のセルは分ける: {table:?}");
    };
    let InlineNode::Quantity { pieces, style } = integer else {
      panic!("integer は Quantity のはず: {integer:?}");
    };
    assert_eq!(pieces, &[QuantityPiece::Text("7".to_string())]);
    assert!(style.color.is_some(), "色は整数部に付く");
    assert!(
      matches!(
        rest.as_slice(),
        [
          InlineNode::Link { .. },
          InlineNode::Footnote { number: 1, .. }
        ]
      ),
      "R は脚注マーカーのリンクと脚注本体の 2 ノードだけ（数量ノードは作らない）: {rest:?}"
    );
  }

  #[test]
  fn lower_table_decimal_span_cell_is_not_split() {
    let nodes = lower_source(
      &ReadStyle::default(),
      "\\begin{table}[columns=decimal decimal]\n\\row{\\cell[span=2]{合計}}\n\\end{table}\n",
    );
    let table = find_table(&nodes);

    assert!(matches!(&table.rows[0].cells[0].content, TableCellContent::Inline(_)));
  }
}
