//! 表エンジン — 表の組版（セルの組版と計測 → 行帯 → 列幅の決定 → 行断片 → 再掲ヘッダ）を閉じる
//!
//! 入口は [`Measurer::build_table`]。入力は lowering が列指定・セルのインライン内容・セル余白・罫線の太さと色まで
//! 解決した `TableLayout` で、HIR の表語彙は届かない。出力 [`TableBox`] は表左端・行帯上端を原点とする行断片の列で、
//! 配置（`breaking::break_pages`）は段オフセットと揃えを足して置くだけ。
//!
//! 列幅の基準 `available` は表を置く段の幅（`BlockBuildInputs::column_width`）。本文では
//! `PreparedGeometry::body_column_width` で、`break_pages` が段幅に使う値と同じ式・同じ入力から出る（前付けは本文幅で、
//! 前付けの段数 1・段間 0 の段幅と一致する）。配置側の揃えはこの段幅と [`TableBox::width`] から決まる。
//!
//! 再掲ヘッダは初出ヘッダの断片から索引語と脚注を除いたもの（同じ語をページごとに積まず、脚注本体は初出の 1 回だけ
//! 置く）。リンクは再掲先にもクリック矩形が要るので残す — 脚注マーカーのリンクも残るので、再掲ヘッダのマーカーは
//! 初出の脚注本体へ飛ぶ。

use crate::{
  document::{ColumnAlign, ColumnWidth},
  length::Length,
  typeset::{
    boxes::{
      HItem, IndexTerm, LineLink, LinkTarget, MeasuredFootnote, PlacedHBox, PlacedTableRow, PlacedTableRule, TableBox,
      TableRowFragment,
    },
    boxing::{self, Measurer, hlist::HList},
    breaking,
    lowering::{TableColumn, TableLayout, TableRowLayout},
  },
};

/// 計測済みの 1 セル
#[derive(Debug)]
struct MeasuredCell {
  /// セル内容の水平アイテム列（セル内では行分割しないので未分割のまま）
  items: Vec<HItem>,
  /// 列方向の結合数（1 以上）
  column_span: u32,
}

/// 計測済みの 1 行。行帯は [`MeasuredRow::new`] が構築時に確定する
#[derive(Debug)]
struct MeasuredRow {
  /// 行内のセル
  cells: Vec<MeasuredCell>,
  /// この行の上に横罫線を引くか
  rule_above: bool,
  /// 行帯の高さ
  height: Length,
  /// 行帯上端からセル内容が共有するベースラインまでの距離
  baseline: Length,
}

impl MeasuredRow {
  /// セル列から行を作り、行帯の高さとベースライン位置を確定する
  ///
  /// 行帯の高さは本文の行送りと同じ `max(F × line_height_factor, h + d)`。F は行内の最大フォントサイズ（テキストが
  /// 無ければ `default_font_size`）、h / d は [`row_fragment`] が描く箱（`HItem::Box`）の高さ・深さの最大値。
  /// 余り `高さ − (h + d)` は上下へ等分するので、ベースラインは行帯上端から `h + 余り / 2` にあり、描く箱は
  /// 行帯の上端と下端の間に収まる。
  fn new(cells: Vec<MeasuredCell>, rule_above: bool, default_font_size: Length, line_height_factor: f32) -> Self {
    let font_size = cells
      .iter()
      .filter_map(|cell| return boxing::max_font_size_in_items(&cell.items))
      .reduce(Length::max)
      .unwrap_or(default_font_size);
    let (ascent, descent) = cells
      .iter()
      .flat_map(|cell| return cell.items.iter())
      .filter_map(|item| match item {
        // `row_fragment` が描く箱だけを数える（描かないアイテムが行帯を広げない）
        HItem::Box(hbox) => return Some(hbox),
        HItem::Glue { .. }
        | HItem::Kern(_)
        | HItem::MathBreak { .. }
        | HItem::Penalty { .. }
        | HItem::Discretionary { .. }
        | HItem::ForcedBreak
        | HItem::LinkStart(_)
        | HItem::LinkEnd
        | HItem::FlushRight(_)
        | HItem::Footnote(_)
        | HItem::IndexMark(_) => return None,
      })
      .fold((Length::ZERO, Length::ZERO), |(ascent, descent), hbox| {
        return (ascent.max(hbox.height), descent.max(hbox.depth));
      });
    let height = (font_size * line_height_factor).max(ascent + descent);
    let baseline = ascent + (height - ascent - descent) / 2.0;
    return MeasuredRow {
      cells,
      rule_above,
      height,
      baseline,
    };
  }
}

/// 表 1 個で固定の見た目（列定義と、lowering が解決したセル余白・罫線）
struct TableFrame<'a> {
  /// 列定義（揃え + 幅指定）
  columns: &'a [TableColumn],
  /// セルの内側余白（左右各）
  cell_padding: Length,
  /// 横罫線の太さ
  rule_thickness: Length,
  /// 横罫線の色（RGB）。`None` は黒
  rule_color: Option<[u8; 3]>,
}

impl Measurer<'_> {
  /// 表を組む: セルを計測し、列幅を `available`（表を置く段の幅）で決め、行断片の列にする
  pub(super) fn build_table(&mut self, table: TableLayout, available: Length) -> TableBox {
    let TableLayout {
      columns,
      head,
      rows,
      breakable,
      cell_padding,
      rule_thickness,
      rule_color,
    } = table;
    let head = self.measure_rows(head);
    let rows = self.measure_rows(rows);
    let frame = TableFrame {
      columns: &columns,
      cell_padding,
      rule_thickness,
      rule_color,
    };
    return arrange(&frame, &head, &rows, available, breakable);
  }

  /// 行のセル内容を水平リストに組んで計測する
  fn measure_rows(&mut self, rows: Vec<TableRowLayout>) -> Vec<MeasuredRow> {
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
      let mut cells = Vec::with_capacity(row.cells.len());
      for cell in row.cells {
        let mut list = HList::default();
        for node in cell.content {
          self.collect_inline(node, &mut list);
        }
        cells.push(MeasuredCell {
          items: list.into_items(),
          column_span: cell.column_span,
        });
      }
      result.push(MeasuredRow::new(cells, row.rule_above, self.default_font_size, self.line_height_factor));
    }
    return result;
  }
}

/// 計測済みの行から列幅を決め、初出ヘッダ・再掲ヘッダ・本体行の行断片を組む（フォントに触れない）
fn arrange(
  frame: &TableFrame<'_>,
  head: &[MeasuredRow],
  rows: &[MeasuredRow],
  available: Length,
  breakable: bool,
) -> TableBox {
  let col_widths = resolve_column_widths(frame, head, rows, available);
  let head: Vec<TableRowFragment> = head.iter().map(|row| return row_fragment(row, frame, &col_widths)).collect();
  let repeated_head = head
    .iter()
    .map(|fragment| {
      return TableRowFragment {
        index_marks: Vec::new(),
        footnotes: Vec::new(),
        ..fragment.clone()
      };
    })
    .collect();
  return TableBox {
    width: col_widths.iter().copied().sum(),
    head,
    repeated_head,
    rows: rows.iter().map(|row| return row_fragment(row, frame, &col_widths)).collect(),
    breakable,
  };
}

/// アイテム列の自然幅を返す
fn measure_items_width(items: &[HItem]) -> Length { return items.iter().map(HItem::natural_width).sum(); }

/// `total`（非負）を `parts`（1 以上）個へ sp 単位で等分し、余りを左の部分から 1sp ずつ配る
///
/// `parts == 0` は呼び出し側が避ける（配り先の列が無いときは呼ばない）。
fn split_evenly(total: Length, parts: usize) -> Vec<Length> {
  let parts = i64::try_from(parts).expect("列数は i64 に収まる");
  let base = total.sp().div_euclid(parts);
  let remainder = total.sp().rem_euclid(parts);
  return (0..parts).map(|k| return Length::from_sp(base + i64::from(k < remainder))).collect();
}

/// 跨ぐ列のどれかが `Auto` 以外か（結合セルが折り返すか）。全列 `Auto` の結合セルは折り返さず、自然幅で列を広げる
fn wraps(columns: &[TableColumn]) -> bool {
  return columns.iter().any(|column| match column.width {
    ColumnWidth::Auto => return false,
    ColumnWidth::Fixed(_) | ColumnWidth::Ratio(_) | ColumnWidth::Flex => return true,
  });
}

/// 列幅を解決する
///
/// 1. `column_span = 1` のセルから、列ごとの自然幅（内容実測 + 左右 padding）と下限（最小内容幅 + 左右 padding）を取る
/// 2. 全列 `Auto` の結合セルは、自然幅の不足分を跨ぐ列の自然幅へ sp 単位で均等に足す
/// 3. `Fixed` は指定値、`Ratio` は `available` 比、`Auto` は `max(自然幅, 左右 padding)`
/// 4. `Flex`（`*`）は `max(available − 3 の合計, 0)` を開いている Flex 列で等分し、下限を下回る列を下限に固定して
///    残りから引くことを、違反が無くなるまで繰り返す
/// 5. `Auto` 以外を跨ぐ結合セルは、跨ぐ列の幅の合計が最小内容幅 + 左右 padding に足りなければ、不足分を跨ぐ `Flex` 列へ、
///    無ければ跨ぐ `Auto` 列へ均等に足す（`Fixed` / `Ratio` は書き換えず、はみ出しは配置が warning にする）
///
/// 合計が `available` を超えても縮小しない（はみ出しは配置が warning にする）。
fn resolve_column_widths(
  frame: &TableFrame<'_>,
  head: &[MeasuredRow],
  rows: &[MeasuredRow],
  available: Length,
) -> Vec<Length> {
  let padding = frame.cell_padding;
  let column_count = frame.columns.len();
  let mut naturals = vec![Length::ZERO; column_count];
  let mut floors = vec![padding * 2; column_count];

  for row in head.iter().chain(rows) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let column_span = cell.column_span as usize;
      if column_span == 1 && column_index < column_count {
        naturals[column_index] = naturals[column_index].max(measure_items_width(&cell.items) + padding * 2);
        floors[column_index] = floors[column_index].max(breaking::min_content_width(&cell.items) + padding * 2);
      }
      column_index += column_span;
    }
  }
  for row in head.iter().chain(rows) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let column_span = cell.column_span as usize;
      let spanned = column_index..column_index + column_span;
      if column_span > 1 && spanned.end <= column_count && !wraps(&frame.columns[spanned.clone()]) {
        let width = measure_items_width(&cell.items) + padding * 2;
        let current: Length = naturals[spanned.clone()].iter().sum();
        if width > current {
          for (natural, extra) in naturals[spanned].iter_mut().zip(split_evenly(width - current, column_span)) {
            *natural += extra;
          }
        }
      }
      column_index += column_span;
    }
  }

  let mut widths = vec![Length::ZERO; column_count];
  let mut open_flex: Vec<usize> = Vec::new();
  let mut used = Length::ZERO;
  for (i, column) in frame.columns.iter().enumerate() {
    match column.width {
      ColumnWidth::Fixed(length) => widths[i] = length,
      ColumnWidth::Ratio(ratio) => widths[i] = available * ratio,
      ColumnWidth::Auto => widths[i] = naturals[i].max(padding * 2),
      ColumnWidth::Flex => {
        open_flex.push(i);
        continue;
      },
    }
    used += widths[i];
  }
  let mut rest = (available - used).max(Length::ZERO);
  while !open_flex.is_empty() {
    let shares = split_evenly(rest, open_flex.len());
    let violates = |k: usize| return shares[k] < floors[open_flex[k]];
    if !(0..open_flex.len()).any(violates) {
      for (&i, share) in open_flex.iter().zip(shares) {
        widths[i] = share;
      }
      break;
    }
    let mut still_open = Vec::new();
    for (k, &i) in open_flex.iter().enumerate() {
      if violates(k) {
        widths[i] = floors[i];
        rest = (rest - floors[i]).max(Length::ZERO);
      } else {
        still_open.push(i);
      }
    }
    open_flex = still_open;
  }

  for row in head.iter().chain(rows) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let column_span = cell.column_span as usize;
      let spanned = column_index..column_index + column_span;
      if column_span > 1 && spanned.end <= column_count && wraps(&frame.columns[spanned.clone()]) {
        let need = breaking::min_content_width(&cell.items) + padding * 2;
        let current: Length = widths[spanned.clone()].iter().sum();
        if current < need {
          let mut flex_columns = Vec::new();
          let mut auto_columns = Vec::new();
          for i in spanned {
            match frame.columns[i].width {
              ColumnWidth::Flex => flex_columns.push(i),
              ColumnWidth::Auto => auto_columns.push(i),
              ColumnWidth::Fixed(_) | ColumnWidth::Ratio(_) => {},
            }
          }
          let targets = if flex_columns.is_empty() {
            auto_columns
          } else {
            flex_columns
          };
          if !targets.is_empty() {
            for (&i, extra) in targets.iter().zip(split_evenly(need - current, targets.len())) {
              widths[i] += extra;
            }
          }
        }
      }
      column_index += column_span;
    }
  }
  return widths;
}

/// 1 行の各セルの内容開始位置（表左端からの相対 x。列揃えと内側余白を適用済み）を順に計算する
fn layout_row_cells<'a>(
  row: &'a MeasuredRow,
  frame: &TableFrame<'_>,
  col_widths: &[Length],
) -> Vec<(&'a MeasuredCell, Length)> {
  let mut column_index = 0usize;
  let mut cell_x = Length::ZERO;
  let mut placements = Vec::with_capacity(row.cells.len());
  for cell in &row.cells {
    let column_span = (cell.column_span as usize).min(col_widths.len().saturating_sub(column_index));
    let band_width: Length = col_widths[column_index..column_index + column_span].iter().copied().sum();
    let content_width = measure_items_width(&cell.items);
    let align = frame.columns.get(column_index).map_or(ColumnAlign::Left, |c| return c.align);
    let content_x = match align {
      ColumnAlign::Left => cell_x + frame.cell_padding,
      ColumnAlign::Center => cell_x + (band_width - content_width) / 2.0,
      ColumnAlign::Right => cell_x + band_width - frame.cell_padding - content_width,
    };
    placements.push((cell, content_x));
    cell_x += band_width;
    column_index += column_span;
  }
  return placements;
}

/// 1 行を、表左端・行帯上端を原点とする行断片にする
///
/// 列揃え・余白・`Kern` / `Glue` のカーソル前進はここで解決する。リンク・索引語・脚注のマーカーは描画箱に含めず、
/// リンクは表左端からの水平範囲、索引語と脚注は出現順で断片へ移す。セルは折り返さないので `LinkStart` / `LinkEnd` は
/// 常に同一セル内で対応が閉じる。
fn row_fragment(row: &MeasuredRow, frame: &TableFrame<'_>, col_widths: &[Length]) -> TableRowFragment {
  let mut boxes = Vec::new();
  let mut links = Vec::new();
  let mut index_marks: Vec<IndexTerm> = Vec::new();
  let mut footnotes: Vec<MeasuredFootnote> = Vec::new();
  for (cell, content_x) in layout_row_cells(row, frame, col_widths) {
    let mut cursor = content_x;
    let mut open: Vec<(LinkTarget, Length)> = Vec::new();
    for item in &cell.items {
      match item {
        HItem::Box(hbox) => {
          boxes.push(PlacedHBox {
            hbox: hbox.clone(),
            dx: cursor,
            dy: Length::ZERO,
          });
          cursor += hbox.width;
        },
        HItem::Kern(value) => cursor += *value,
        HItem::MathBreak { spacing, .. } => cursor += *spacing,
        HItem::Glue { natural, .. } => cursor += *natural,
        HItem::LinkStart(target) => open.push((target.clone(), cursor)),
        HItem::LinkEnd => {
          if let Some((target, x0)) = open.pop() {
            links.push(LineLink {
              target,
              x0,
              x1: cursor,
            });
          }
        },
        HItem::IndexMark(term) => index_marks.push(term.clone()),
        HItem::Footnote(footnote) => footnotes.push(footnote.clone()),
        HItem::Penalty { .. } | HItem::Discretionary { .. } | HItem::ForcedBreak | HItem::FlushRight(_) => {},
      }
    }
  }
  let rule = row.rule_above.then_some(PlacedTableRule {
    x: Length::ZERO,
    y: Length::ZERO,
    width: col_widths.iter().copied().sum(),
    height: frame.rule_thickness,
    color: frame.rule_color,
  });
  return TableRowFragment {
    row: PlacedTableRow {
      top_y: Length::ZERO,
      height: row.height,
      baseline_y: row.baseline,
      boxes,
      rule,
    },
    links,
    index_marks,
    footnotes,
  };
}

#[cfg(test)]
mod tests {
  use super::{
    MeasuredCell, MeasuredRow, TableFrame, arrange, measure_items_width, resolve_column_widths, row_fragment,
    split_evenly,
  };
  use crate::{
    document::{ColumnAlign, ColumnWidth, NodeId},
    length::Length,
    project::FontType,
    publication::GlyphRun,
    semantics::LabelId,
    source::SourceId,
    typeset::{
      boxes::{AnchorId, FootnoteId, HBox, HBoxContent, HItem, IndexTerm, LinkTarget, MeasuredFootnote},
      lowering::TableColumn,
    },
  };

  /// pt 値から `Length` を作る
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 指定幅・指定フォントサイズの Glyphs ボックスを作る
  fn glyph_box(width: f32, font_size: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: pt(font_size),
        text: "x".to_string(),
        glyphs: Vec::new(),
        font_type: FontType::Serif,
        color: None,
      }),
      width: pt(width),
      height: pt(font_size),
      depth: Length::ZERO,
    });
  }

  /// 指定幅の、テキストを含まないボックスを作る
  fn text_free_box(width: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(1.0),
      depth: Length::ZERO,
    });
  }

  /// テキストを含まない、指定の高さ・深さのボックスを作る
  fn extent_box(height: f32, depth: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(5.0),
      height: pt(height),
      depth: pt(depth),
    });
  }

  /// 索引語 `word`（読みなし）のマーカーを作る
  fn index_mark(word: &str) -> HItem {
    return HItem::IndexMark(IndexTerm {
      word: word.to_string(),
      reading: None,
    });
  }

  /// 出現 index `index`（表示番号 `index + 1`）の脚注マーカーを作る
  fn footnote(index: u32) -> HItem {
    return HItem::Footnote(MeasuredFootnote {
      origin: NodeId::for_test(SourceId::new(0), index + 1),
      number: index + 1,
      index,
      items: vec![text_free_box(30.0)],
      leading: pt(10.0),
    });
  }

  /// 索引語の列から語だけを取り出す
  fn words(terms: &[IndexTerm]) -> Vec<&str> { return terms.iter().map(|term| return term.word.as_str()).collect(); }

  /// `column_span = 1` のセルを作る
  fn cell(items: Vec<HItem>) -> MeasuredCell {
    return MeasuredCell {
      items,
      column_span: 1,
    };
  }

  /// `rule_above = false` の行を作る（既定フォントサイズ 10pt・行高係数 1）
  fn row(cells: Vec<MeasuredCell>) -> MeasuredRow { return MeasuredRow::new(cells, false, pt(10.0), 1.0); }

  /// 揃え `align`・幅指定 `Auto` の列を作る
  fn auto(align: ColumnAlign) -> TableColumn {
    return TableColumn {
      align,
      width: ColumnWidth::Auto,
    };
  }

  /// 幅 5 の breakable glue（語間の分割機会）
  fn space() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: Length::ZERO,
      shrink: Length::ZERO,
      breakable: true,
    };
  }

  /// 指定幅（sp まで正確）の、テキストを含まないボックスを作る
  fn box_of(width: Length) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width,
      height: pt(1.0),
      depth: Length::ZERO,
    });
  }

  /// 揃え `align`・幅指定 `Flex` の列を作る
  fn flex(align: ColumnAlign) -> TableColumn {
    return TableColumn {
      align,
      width: ColumnWidth::Flex,
    };
  }

  /// 幅指定 `Fixed(width)`（pt）の左揃えの列を作る
  fn fixed(width: f32) -> TableColumn {
    return TableColumn {
      align: ColumnAlign::Left,
      width: ColumnWidth::Fixed(pt(width)),
    };
  }

  /// 幅指定 `Ratio(ratio)` の左揃えの列を作る
  fn ratio(ratio: f32) -> TableColumn {
    return TableColumn {
      align: ColumnAlign::Left,
      width: ColumnWidth::Ratio(ratio),
    };
  }

  /// 列方向に `span` 列を結合したセルを作る
  fn span_cell(items: Vec<HItem>, span: u32) -> MeasuredCell {
    return MeasuredCell {
      items,
      column_span: span,
    };
  }

  /// 幅を sp の列で返す
  fn sps(widths: &[Length]) -> Vec<i64> { return widths.iter().map(|width| return width.sp()).collect(); }

  /// セル余白 `padding`・罫線なし（太さ 0・黒）の表の見た目を作る
  fn frame(columns: &[TableColumn], padding: f32) -> TableFrame<'_> {
    return TableFrame {
      columns,
      cell_padding: pt(padding),
      rule_thickness: Length::ZERO,
      rule_color: None,
    };
  }

  #[test]
  fn measure_items_width_is_additive() {
    let items = vec![
      text_free_box(10.0),
      HItem::Glue {
        natural: pt(5.0),
        stretch: Length::ZERO,
        shrink: Length::ZERO,
        breakable: true,
      },
      HItem::Kern(pt(3.0)),
    ];

    assert!(close(measure_items_width(&items), 18.0));
    assert!(close(measure_items_width(&[HItem::Penalty { value: 0 }, HItem::ForcedBreak]), 0.0));
  }

  #[test]
  fn row_band_keeps_the_leading_and_centers_short_content() {
    // 中身は高さ 12・深さ 2 で計 14、行送り 12 × 1.5 = 18 に収まる。余り 4 を上下へ 2 ずつ
    let row = MeasuredRow::new(
      vec![
        cell(vec![glyph_box(10.0, 12.0)]),
        cell(vec![extent_box(3.0, 2.0)]),
      ],
      false,
      pt(9.0),
      1.5,
    );

    assert!(close(row.height, 18.0), "{row:?}");
    assert!(close(row.baseline, 14.0), "{row:?}");
  }

  #[test]
  fn row_band_grows_to_the_content_extent() {
    // 中身は高さ 14・深さ 7 で計 21、行送り 12 × 1.2 = 14.4 を超えるので行帯は中身に一致する
    let row = MeasuredRow::new(
      vec![
        cell(vec![glyph_box(10.0, 12.0)]),
        cell(vec![extent_box(14.0, 7.0)]),
      ],
      false,
      pt(9.0),
      1.2,
    );

    assert!(close(row.height, 21.0), "{row:?}");
    assert!(close(row.baseline, 14.0), "余りが無いのでベースラインは上端 + 高さ: {row:?}");
  }

  #[test]
  fn row_band_without_text_uses_the_default_font_size() {
    // テキストが無いので行送りは既定 9 × 2 = 18。中身は高さ 1 で、余り 17 を上下へ 8.5 ずつ
    let row = MeasuredRow::new(vec![cell(vec![text_free_box(5.0)])], false, pt(9.0), 2.0);

    assert!(close(row.height, 18.0), "{row:?}");
    assert!(close(row.baseline, 9.5), "{row:?}");
  }

  #[test]
  fn resolve_column_widths_fixed_auto_ratio() {
    let columns = [
      TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Fixed(Length::pt(40.0)),
      },
      auto(ColumnAlign::Left),
      TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Ratio(0.5),
      },
    ];
    let rows = [row(vec![
      cell(vec![text_free_box(5.0)]),
      cell(vec![text_free_box(30.0)]),
      cell(vec![text_free_box(5.0)]),
    ])];

    let widths = resolve_column_widths(&frame(&columns, 2.0), &[], &rows, pt(200.0));

    assert!(close(widths[0], 40.0), "Fixed: {widths:?}");
    assert!(close(widths[1], 34.0), "Auto=内容+2*padding: {widths:?}");
    assert!(close(widths[2], 100.0), "Ratio=比*available: {widths:?}");
  }

  #[test]
  fn resolve_column_widths_flex_shares_remaining() {
    let columns = [
      auto(ColumnAlign::Left),
      TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Flex,
      },
    ];
    let rows = [row(vec![
      cell(vec![text_free_box(20.0)]),
      cell(vec![text_free_box(10.0)]),
    ])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(100.0));

    assert!(close(widths[0], 20.0), "{widths:?}");
    assert!(close(widths[1], 80.0), "Flex=残り幅の等分: {widths:?}");
  }

  #[test]
  fn split_evenly_gives_the_remainder_to_the_left_parts() {
    assert_eq!(sps(&split_evenly(Length::from_sp(10), 3)), [4, 3, 3]);
  }

  #[test]
  fn flex_shares_the_rest_instead_of_growing_to_the_natural_width() {
    let columns = [auto(ColumnAlign::Left), flex(ColumnAlign::Left)];
    let rows = [row(vec![
      cell(vec![text_free_box(60.0)]),
      cell(vec![
        text_free_box(20.0),
        space(),
        text_free_box(20.0),
        space(),
        text_free_box(30.0),
      ]),
    ])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(100.0));

    assert_eq!(widths, [pt(60.0), pt(40.0)]);
  }

  #[test]
  fn flex_floor_pins_the_column_and_redistributes() {
    let columns = [flex(ColumnAlign::Left); 3];
    let rows = [row(vec![
      cell(vec![text_free_box(50.0)]),
      cell(vec![text_free_box(5.0)]),
      cell(vec![text_free_box(5.0)]),
    ])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(90.0));

    assert_eq!(widths, [pt(50.0), pt(20.0), pt(20.0)]);
  }

  #[test]
  fn flex_floor_includes_the_cell_padding() {
    let columns = [flex(ColumnAlign::Left)];
    let rows = [row(vec![cell(vec![text_free_box(10.0)])])];

    let widths = resolve_column_widths(&frame(&columns, 2.0), &[], &rows, pt(5.0));

    assert_eq!(widths, [pt(14.0)]);
  }

  #[test]
  fn flex_falls_to_its_floor_when_fixed_columns_use_up_the_width() {
    let columns = [fixed(120.0), flex(ColumnAlign::Left)];
    let rows = [row(vec![
      cell(vec![text_free_box(5.0)]),
      cell(vec![text_free_box(8.0)]),
    ])];

    let widths = resolve_column_widths(&frame(&columns, 1.0), &[], &rows, pt(100.0));

    assert_eq!(widths, [pt(120.0), pt(10.0)]);
  }

  #[test]
  fn flex_remainder_goes_to_the_left_columns() {
    let columns = [flex(ColumnAlign::Left); 3];
    let rows = [row(vec![
      cell(Vec::new()),
      cell(Vec::new()),
      cell(Vec::new()),
    ])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, Length::from_sp(10));

    assert_eq!(sps(&widths), [4, 3, 3]);
  }

  #[test]
  fn all_auto_span_shortfall_is_split_to_the_sp() {
    let columns = [auto(ColumnAlign::Left); 2];
    let rows = [
      row(vec![cell(Vec::new()), cell(Vec::new())]),
      row(vec![span_cell(vec![box_of(Length::from_sp(5))], 2)]),
    ];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(100.0));

    assert_eq!(sps(&widths), [3, 2]);
  }

  #[test]
  fn span_over_fixed_and_flex_gives_the_min_content_shortfall_to_flex() {
    let columns = [fixed(20.0), flex(ColumnAlign::Left)];
    let rows = [
      row(vec![cell(Vec::new()), cell(Vec::new())]),
      row(vec![span_cell(vec![text_free_box(50.0)], 2)]),
    ];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(30.0));

    assert_eq!(widths, [pt(20.0), pt(30.0)]);
  }

  #[test]
  fn span_shortfall_goes_to_auto_when_no_flex_is_spanned() {
    let columns = [fixed(20.0), auto(ColumnAlign::Left)];
    let rows = [
      row(vec![cell(Vec::new()), cell(vec![text_free_box(5.0)])]),
      row(vec![span_cell(vec![text_free_box(40.0)], 2)]),
    ];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(100.0));

    assert_eq!(widths, [pt(20.0), pt(20.0)]);
  }

  #[test]
  fn span_over_fixed_and_ratio_keeps_the_specified_widths() {
    let columns = [fixed(20.0), ratio(0.1)];
    let rows = [row(vec![span_cell(vec![text_free_box(50.0)], 2)])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(100.0));

    assert_eq!(widths, [pt(20.0), pt(10.0)]);
  }

  #[test]
  fn span_that_fits_by_min_content_does_not_grow() {
    // 自然幅 21 は跨ぐ列の合計 30 に収まらないが、最小内容幅 8 は収まるので足さない
    let columns = [fixed(20.0), flex(ColumnAlign::Left)];
    let rows = [row(vec![span_cell(
      vec![text_free_box(8.0), space(), text_free_box(8.0)],
      2,
    )])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &[], &rows, pt(30.0));

    assert_eq!(widths, [pt(20.0), pt(10.0)]);
  }

  #[test]
  fn resolve_column_widths_reads_head_rows_too() {
    // ヘッダ行のセルのほうが広ければ、列の自然幅はヘッダで決まる
    let columns = [auto(ColumnAlign::Left)];
    let head = [row(vec![cell(vec![text_free_box(50.0)])])];
    let rows = [row(vec![cell(vec![text_free_box(10.0)])])];

    let widths = resolve_column_widths(&frame(&columns, 0.0), &head, &rows, pt(200.0));

    assert!(close(widths[0], 50.0), "{widths:?}");
  }

  #[test]
  fn resolve_column_widths_spreads_span_shortfall_evenly() {
    // 跨ぐ列の自然幅を 12 / 22 と不揃いにし、均等配分と比例配分を区別する。span セルは列 1 から始める
    let columns = [auto(ColumnAlign::Left); 3];
    let rows = [
      row(vec![
        cell(vec![text_free_box(5.0)]),
        cell(vec![text_free_box(10.0)]),
        cell(vec![text_free_box(20.0)]),
      ]),
      row(vec![
        cell(vec![text_free_box(5.0)]),
        MeasuredCell {
          items: vec![text_free_box(50.0)],
          column_span: 2,
        },
      ]),
    ];

    let widths = resolve_column_widths(&frame(&columns, 1.0), &[], &rows, pt(200.0));

    assert!(close(widths[0], 7.0), "span が跨がない列は変わらない: {widths:?}");
    assert!(close(widths[1], 21.0), "不足 18 の半分を足す: {widths:?}");
    assert!(close(widths[2], 31.0), "不足 18 の半分を足す: {widths:?}");
  }

  #[test]
  fn resolve_column_widths_keeps_naturals_when_span_fits() {
    let columns = [auto(ColumnAlign::Left); 3];
    let rows = [
      row(vec![
        cell(vec![text_free_box(5.0)]),
        cell(vec![text_free_box(10.0)]),
        cell(vec![text_free_box(20.0)]),
      ]),
      row(vec![
        cell(vec![text_free_box(5.0)]),
        MeasuredCell {
          items: vec![text_free_box(20.0)],
          column_span: 2,
        },
      ]),
    ];

    let widths = resolve_column_widths(&frame(&columns, 1.0), &[], &rows, pt(200.0));

    assert!(close(widths[0], 7.0), "{widths:?}");
    assert!(close(widths[1], 12.0), "跨ぐ列の合計 34 が span セル幅 22 を満たすので配分しない: {widths:?}");
    assert!(close(widths[2], 22.0), "跨ぐ列の合計 34 が span セル幅 22 を満たすので配分しない: {widths:?}");
  }

  #[test]
  fn row_fragment_resolves_alignment_and_spacing() {
    let row = row(vec![cell(vec![
      text_free_box(5.0),
      HItem::Kern(pt(3.0)),
      HItem::Glue {
        natural: pt(2.0),
        stretch: Length::ZERO,
        shrink: Length::ZERO,
        breakable: false,
      },
      text_free_box(4.0),
    ])]);
    let columns = [auto(ColumnAlign::Right)];

    let boxes = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)]).row.boxes;

    assert_eq!(boxes.len(), 2);
    assert!(close(boxes[0].dx, 14.0), "右揃えの先頭 x: {boxes:?}");
    assert!(close(boxes[1].dx, 24.0), "box + kern + glue 後の x: {boxes:?}");
  }

  #[test]
  fn row_fragment_aligns_spanned_cell_within_band() {
    // 揃えは開始列（Center）で決まり、被覆列（Left）は読まない
    let row = row(vec![
      MeasuredCell {
        items: vec![text_free_box(10.0)],
        column_span: 2,
      },
      cell(vec![text_free_box(4.0)]),
    ]);
    let columns = [
      auto(ColumnAlign::Center),
      auto(ColumnAlign::Left),
      auto(ColumnAlign::Right),
    ];

    let boxes = row_fragment(&row, &frame(&columns, 2.0), &[pt(20.0), pt(30.0), pt(25.0)]).row.boxes;

    assert_eq!(boxes.len(), 2);
    assert!(close(boxes[0].dx, 20.0), "帯幅 50 の中央: {boxes:?}");
    assert!(close(boxes[1].dx, 69.0), "後続セルは帯の右端 50 から始まる: {boxes:?}");
  }

  #[test]
  fn row_fragment_places_rule_and_padding_from_the_frame() {
    let columns = [auto(ColumnAlign::Left)];
    let frame = TableFrame {
      columns: &columns,
      cell_padding: pt(3.0),
      rule_thickness: pt(1.5),
      rule_color: Some([9, 9, 9]),
    };
    let row_with_rule = MeasuredRow::new(vec![cell(vec![glyph_box(10.0, 10.0)])], true, pt(10.0), 1.0);
    let plain_row = row(vec![cell(vec![glyph_box(10.0, 10.0)])]);

    let fragment = row_fragment(&row_with_rule, &frame, &[pt(30.0)]);

    assert!(close(fragment.row.top_y, 0.0), "{fragment:?}");
    assert!(close(fragment.row.height, row_with_rule.height.to_pt()), "{fragment:?}");
    assert!(close(fragment.row.baseline_y, row_with_rule.baseline.to_pt()), "{fragment:?}");
    assert!(close(fragment.row.boxes[0].dx, 3.0), "表左端 + 余白: {fragment:?}");
    let rule = fragment.row.rule.expect("rule_above の行は罫線を持つ");
    assert!(close(rule.x, 0.0) && close(rule.y, 0.0), "{rule:?}");
    assert!(close(rule.width, 30.0) && close(rule.height, 1.5), "{rule:?}");
    assert_eq!(rule.color, Some([9, 9, 9]));
    assert!(row_fragment(&plain_row, &frame, &[pt(30.0)]).row.rule.is_none(), "rule_above でない行は罫線なし");
  }

  #[test]
  fn row_fragment_collects_index_marks_in_cell_order() {
    let row = row(vec![
      cell(vec![text_free_box(5.0), index_mark("a")]),
      cell(vec![index_mark("b"), text_free_box(5.0)]),
    ]);
    let columns = [auto(ColumnAlign::Left); 2];

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(10.0), pt(10.0)]);

    assert_eq!(words(&fragment.index_marks), vec!["a", "b"]);
    assert_eq!(fragment.row.boxes.len(), 2, "索引語は描画箱を持たない");
  }

  #[test]
  fn row_fragment_collects_footnotes_in_cell_order() {
    // 左のセルに 1 つ、右のセルに 2 つ（箱の前と後）
    let row = row(vec![
      cell(vec![text_free_box(5.0), footnote(0)]),
      cell(vec![footnote(1), text_free_box(5.0), footnote(2)]),
    ]);
    let columns = [auto(ColumnAlign::Left); 2];

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(10.0), pt(10.0)]);

    let indices: Vec<u32> = fragment.footnotes.iter().map(|footnote| return footnote.index).collect();
    assert_eq!(indices, vec![0, 1, 2]);
    assert_eq!(fragment.row.boxes.len(), 2, "脚注は描画箱を持たない");
  }

  #[test]
  fn row_fragment_links_single_link_fills_cell() {
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(target.clone()),
      text_free_box(20.0),
      HItem::LinkEnd,
    ])]);
    let columns = [auto(ColumnAlign::Left)];

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)]).links;

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x0, 2.0));
    assert!(close(links[0].x1, 22.0));
  }

  #[test]
  fn row_fragment_links_multiple_links_in_one_cell() {
    let first = LinkTarget::Internal(AnchorId::Label(LabelId::new("fig:1")));
    let second = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(first.clone()),
      text_free_box(5.0),
      HItem::LinkEnd,
      text_free_box(3.0),
      HItem::LinkStart(second.clone()),
      text_free_box(5.0),
      HItem::LinkEnd,
    ])]);
    let columns = [auto(ColumnAlign::Left)];

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)]).links;

    assert_eq!(links.len(), 2);
    assert_eq!(links[0].target, first);
    assert!(close(links[0].x0, 0.0) && close(links[0].x1, 5.0));
    assert_eq!(links[1].target, second);
    assert!(close(links[1].x0, 8.0) && close(links[1].x1, 13.0));
  }

  #[test]
  fn row_fragment_links_ignore_unbalanced_link_end() {
    let target = LinkTarget::Internal(AnchorId::Label(LabelId::new("x")));
    let row = row(vec![cell(vec![
      HItem::LinkStart(target.clone()),
      HItem::LinkEnd,
      HItem::LinkEnd,
    ])]);
    let columns = [auto(ColumnAlign::Left)];

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)]).links;

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x0, 0.0) && close(links[0].x1, 0.0));
  }

  #[test]
  fn row_fragment_links_respect_column_alignment() {
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(target),
      text_free_box(10.0),
      HItem::LinkEnd,
    ])]);
    let columns = [auto(ColumnAlign::Right)];

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)]).links;

    assert!(close(links[0].x0, 18.0) && close(links[0].x1, 28.0), "{links:?}");
  }

  #[test]
  fn row_fragment_links_in_spanned_cell_follow_band() {
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![MeasuredCell {
      items: vec![
        HItem::LinkStart(target.clone()),
        text_free_box(10.0),
        HItem::LinkEnd,
      ],
      column_span: 2,
    }]);
    let columns = [auto(ColumnAlign::Right), auto(ColumnAlign::Left)];

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(20.0), pt(30.0)]).links;

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x0, 38.0) && close(links[0].x1, 48.0), "帯幅 50 の右揃え: {links:?}");
  }

  #[test]
  fn arrange_repeated_head_drops_index_marks_and_keeps_links() {
    let head_row = row(vec![cell(vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      text_free_box(10.0),
      HItem::LinkEnd,
      index_mark("h"),
    ])]);
    let body_row = row(vec![cell(vec![text_free_box(10.0)])]);
    let columns = [auto(ColumnAlign::Left)];

    let table = arrange(&frame(&columns, 2.0), &[head_row], &[body_row], pt(100.0), true);

    assert_eq!(words(&table.head[0].index_marks), vec!["h"], "初出のヘッダは索引語を持つ");
    assert!(table.repeated_head[0].index_marks.is_empty(), "再掲ヘッダは索引語を重複させない");
    assert_eq!(table.repeated_head[0].links.len(), 1, "再掲ヘッダにもリンク矩形が要る");
    assert_eq!(table.repeated_head[0].row.boxes.len(), table.head[0].row.boxes.len());
    assert_eq!(table.rows.len(), 1);
    assert!(table.breakable);
  }

  #[test]
  fn arrange_repeated_head_drops_footnotes_and_keeps_links() {
    // 脚注マーカーと同じ形（リンクで包んだ箱 + 運搬マーカー）のヘッダ 1 行
    let target = LinkTarget::Internal(AnchorId::Footnote(FootnoteId::new(0)));
    let head = vec![row(vec![cell(vec![
      HItem::LinkStart(target.clone()),
      text_free_box(5.0),
      HItem::LinkEnd,
      footnote(0),
    ])])];
    let columns = [auto(ColumnAlign::Left)];

    let table = arrange(&frame(&columns, 0.0), &head, &[], pt(100.0), true);

    assert_eq!(table.head[0].footnotes.len(), 1, "初出ヘッダは脚注を持つ");
    assert!(table.repeated_head[0].footnotes.is_empty(), "再掲ヘッダは脚注を持たない");
    assert_eq!(table.repeated_head[0].links.len(), 1, "再掲ヘッダもマーカーのリンクを持つ");
    assert_eq!(table.repeated_head[0].links[0].target, target);
  }

  #[test]
  fn arrange_width_is_the_sum_of_column_widths() {
    // Fixed 40 + Auto（内容 30 + 余白 2 × 2 = 34）
    let columns = [
      TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Fixed(pt(40.0)),
      },
      auto(ColumnAlign::Left),
    ];
    let rows = [row(vec![
      cell(vec![text_free_box(5.0)]),
      cell(vec![text_free_box(30.0)]),
    ])];

    let table = arrange(&frame(&columns, 2.0), &[], &rows, pt(200.0), false);

    assert!(close(table.width, 74.0), "{table:?}");
    assert!(!table.breakable);
  }
}
