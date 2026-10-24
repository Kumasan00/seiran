//! 表エンジン — 表の組版（セルの組版と計測 → 列幅の決定 → セルの折り返しと行帯 → 行断片 → 行の集まりの畳み込み →
//! 再掲ヘッダ）を閉じる
//!
//! 入口は [`Measurer::build_table`]。入力は lowering が列指定・セルのインライン内容・セル余白・罫線の太さと空きと色
//! まで解決した `TableLayout` で、HIR の表語彙は届かない。出力 [`TableBox`] は表左端・行帯上端を原点とする行の集まり
//! （行結合でつながる行。結合が無ければ 1 行）の断片の列と、
//! 罫線と空きを確定した境界（断片の上端・下端の枠罫、ヘッダ下の罫線、各行の上の区切りと部分罫。規則は `arrange` の
//! doc）で、配置（`breaking::break_pages`）は段オフセットと揃えを足して置くだけ。
//!
//! 列幅の基準 `available` は表を置く段の幅（`BlockBuildInputs::column_width`）。本文では
//! `PreparedGeometry::body_column_width` で、`break_pages` が段幅に使う値と同じ式・同じ入力から出る（前付けは本文幅で、
//! 前付けの段数 1・段間 0 の段幅と一致する）。配置側の揃えはこの段幅と [`TableBox::width`] から決まる。
//!
//! 跨ぐ列のどれかが `Auto` 以外のセルは、帯幅から左右の余白を除いた内容幅で本文の段落と同じ行分割器
//! （`BlockBuildInputs::breaker`）が折り返す。全列 `Auto` のセルは折り返さず、自然幅の 1 行のまま置く。
//!
//! 再掲ヘッダは初出ヘッダの断片から索引語と脚注を除いたもの（同じ語をページごとに積まず、脚注本体は初出の 1 回だけ
//! 置く）。リンクは再掲先にもクリック矩形が要るので残す — 脚注マーカーのリンクも残るので、再掲ヘッダのマーカーは
//! 初出の脚注本体へ飛ぶ。

use std::ops::Range;

use crate::{
  document::{ColumnAlign, ColumnWidth, NodeId, RowSeparator, TextAlignment},
  length::Length,
  typeset::{
    boxes::{
      HItem, Line, LineLink, LinkTarget, PlacedHBox, PlacedLink, PlacedTableRow, TableBoundary, TableBox,
      TableRowFragment,
    },
    boxing::{self, Measurer, hlist::HList},
    breaking::{self, LineBreaker},
    lowering::{TableColumn, TableLayout, TableRowLayout},
  },
};

/// 計測済みの 1 セル
#[derive(Debug)]
struct MeasuredCell {
  /// セル内容の水平アイテム列（未分割。折り返すセルは [`row_fragment`] が列幅で行に分ける）
  items: Vec<HItem>,
  /// 列方向の結合数（1 以上）
  column_span: u32,
  /// 行方向の結合数（1 以上）
  row_span: u32,
  /// 跨ぐ列の範囲の下に部分罫を引くか
  rule_below: bool,
}

/// 計測済みの 1 行。行帯はセルの行組みと一緒に [`row_fragment`] が決める
#[derive(Debug)]
struct MeasuredRow {
  /// `\row`
  origin: NodeId,
  /// 行内のセル
  cells: Vec<MeasuredCell>,
  /// この行の上の境界に置く区切り
  separator: Option<RowSeparator>,
  /// セルの各行の帯の下限（行送り）
  leading: Length,
}

impl MeasuredRow {
  /// セル列から行を作り、行送りを確定する
  ///
  /// 行送りは本文と同じ `F × line_height_factor` で、F は行内の全セルの最大フォントサイズ（テキストが無ければ
  /// `default_font_size`）。行送りは行単位で 1 つで、セルの各行で変えない。
  fn new(
    origin: NodeId,
    cells: Vec<MeasuredCell>,
    separator: Option<RowSeparator>,
    default_font_size: Length,
    line_height_factor: f32,
  ) -> Self {
    let font_size = cells
      .iter()
      .filter_map(|cell| return boxing::max_font_size_in_items(&cell.items))
      .reduce(Length::max)
      .unwrap_or(default_font_size);
    return MeasuredRow {
      origin,
      cells,
      separator,
      leading: font_size * line_height_factor,
    };
  }
}

/// 表 1 個で固定の見た目（列定義と、lowering が解決したセル余白・罫線）
struct TableFrame<'a> {
  /// 列定義（揃え + 幅指定）
  columns: &'a [TableColumn],
  /// セルの内側余白（左右各）
  cell_padding: Length,
  /// 上端・下端の枠罫の太さ
  outer_rule_thickness: Length,
  /// ヘッダ下の罫線と行群の区切りの罫線の太さ
  rule_thickness: Length,
  /// 罫線と行の間の空き
  rule_gap: Length,
  /// 行群の区切りの空き
  row_gap: Length,
  /// 部分罫の太さ
  partial_rule_thickness: Length,
  /// 部分罫の左右の詰め幅（片側）
  partial_rule_trim: Length,
  /// 横罫線の色（RGB）。`None` は黒
  rule_color: Option<[u8; 3]>,
}

impl Measurer<'_> {
  /// 表を組む: セルを計測し、列幅を `available`（表を置く段の幅）で決め、`breaker` でセルを折り返して行断片の列にする
  pub(super) fn build_table(&mut self, table: TableLayout, available: Length, breaker: &dyn LineBreaker) -> TableBox {
    let TableLayout {
      origin,
      columns,
      head,
      rows,
      breakable,
      cell_padding,
      outer_rule_thickness,
      rule_thickness,
      rule_gap,
      row_gap,
      partial_rule_thickness,
      partial_rule_trim,
      rule_color,
    } = table;
    let head = self.measure_rows(head);
    let rows = self.measure_rows(rows);
    let frame = TableFrame {
      columns: &columns,
      cell_padding,
      outer_rule_thickness,
      rule_thickness,
      rule_gap,
      row_gap,
      partial_rule_thickness,
      partial_rule_trim,
      rule_color,
    };
    return arrange(origin, &frame, &head, &rows, available, breakable, breaker);
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
          row_span: cell.row_span,
          rule_below: cell.rule_below,
        });
      }
      result.push(MeasuredRow::new(row.origin, cells, row.separator, self.default_font_size, self.line_height_factor));
    }
    return result;
  }
}

/// 計測済みの行から列幅を決め、初出ヘッダ・再掲ヘッダ・本体行の行断片と枠の境界を組む（フォントに触れない）
///
/// 罫線の境界（g = `rule_gap`）は、断片の上端が `outer_rule_thickness` の罫線 + 下に g、下端が上に g + 罫線、
/// ヘッダ下（`\head` があるときだけ）と `separator=rule` の行の上が g + `rule_thickness` の罫線 + g、`separator=gap`
/// の行の上が `row_gap` の空き。太さ 0 の罫線は空きも取らない（[`TableBoundary::rule`]）。表の最初の行（`head[0]`、
/// ヘッダが無ければ `rows[0]`）の上は枠罫の持ち場で何も無い境界、ヘッダがあるときの `rows[0]` の上はヘッダ下の罫線で、
/// どちらも行自身の区切りを見ない（この 2 つの境界への `separator` は frontend が拒否する）。
///
/// それ以外の行の上は、同じ行群（ヘッダか本体）で結合の最後の行が 1 つ前の行であるセル（結合の無いセルは 1 つ前の行
/// のセル）に `rule_below` があれば部分罫の境界 — g + `partial_rule_thickness` + g で、旗を持つセルの帯ごとに左右を
/// `partial_rule_trim` ずつ詰めた罫線を同じ高さに並べる（[`TableBoundary::partial_rules`]）。結合セルの部分罫は結合の
/// 最後の行の下に引く。結合の最後の行が行群の最後の行である旗と、その次の行が `separator` を持つ旗は frontend が
/// 拒否するので、旗は行群の外へ写らず、区切りと重ならない。
///
/// 行は行群ごとに行の集まり（[`row_groups`]: 行結合がまたぐ境界の上下の行を同じ集まりに入れた極大の連続範囲）に分け、
/// 集まりごとに 1 つの断片へ畳む（[`merge_row_group`]）— 配置は断片を分割しないので、結合した行はいっしょに改ページ
/// する。内側の境界（部分罫だけ。`separator` は frontend の `TableSeparatorInsideRowSpan` が拒否する）は断片の高さに
/// 入り、罫線は断片の `rules` へ移る。
fn arrange(
  origin: NodeId,
  frame: &TableFrame<'_>,
  head: &[MeasuredRow],
  rows: &[MeasuredRow],
  available: Length,
  breakable: bool,
  breaker: &dyn LineBreaker,
) -> TableBox {
  let col_widths = resolve_column_widths(frame, head, rows, available);
  let width: Length = col_widths.iter().copied().sum();
  let rule = |gap_above: Length, thickness: Length, gap_below: Length| {
    return TableBoundary::rule(gap_above, thickness, gap_below, width, frame.rule_color);
  };
  let separator = rule(frame.rule_gap, frame.rule_thickness, frame.rule_gap);
  let head_bottom = if head.is_empty() {
    TableBoundary::default()
  } else {
    separator.clone()
  };
  let fragments = |rows: &[MeasuredRow], first_above: &TableBoundary| {
    let mut parts = rows.iter().enumerate().map(|(index, row)| {
      let above = if index == 0 {
        first_above.clone()
      } else {
        boundary_above(rows, index, &separator, frame, &col_widths)
      };
      let (fragment, merged) = row_fragment(row, frame, &col_widths, breaker);
      return (TableRowFragment { above, ..fragment }, merged);
    });
    return row_groups(rows)
      .into_iter()
      .map(|group| {
        if rows[group.clone()].iter().skip(1).any(|row| return row.separator.is_some()) {
          unreachable!("行結合の内側の境界の separator は frontend（TableSeparatorInsideRowSpan）が拒否する");
        }
        return merge_row_group(parts.by_ref().take(group.len()).collect());
      })
      .collect::<Vec<_>>();
  };
  let head = fragments(head, &TableBoundary::default());
  let rows = fragments(rows, &head_bottom);
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
    origin,
    width,
    top: rule(Length::ZERO, frame.outer_rule_thickness, frame.rule_gap),
    head_bottom,
    bottom: rule(frame.rule_gap, frame.outer_rule_thickness, Length::ZERO),
    head,
    repeated_head,
    rows,
    breakable,
  };
}

/// 行群 `rows` の行 `index`（1 以上）の上の境界 — 結合の最後の行がその直前の行である `rule_below` セル（行 j < `index`
/// で `j + row_span == index`）の部分罫か、行 `index` の区切り（`rule` は `separator`、`gap` は `row_gap` の空き）か、
/// 何も無い境界
fn boundary_above(
  rows: &[MeasuredRow],
  index: usize,
  separator: &TableBoundary,
  frame: &TableFrame<'_>,
  col_widths: &[Length],
) -> TableBoundary {
  let ends_above =
    |start: usize, cell: &MeasuredCell| return cell.rule_below && start + cell.row_span as usize == index;
  let bands: Vec<(Length, Length)> = rows[..index]
    .iter()
    .enumerate()
    .filter(|(start, row)| return row.cells.iter().any(|cell| return ends_above(*start, cell)))
    .flat_map(|(start, row)| {
      return cell_bands(&row.cells, col_widths)
        .into_iter()
        .filter(move |(cell, ..)| return ends_above(start, cell))
        .map(|(_, _, x, band_width)| return (x, band_width));
    })
    .collect();
  return match (bands.is_empty(), rows[index].separator) {
    (false, Some(_)) => {
      unreachable!("rule_below の行の次の行の separator は frontend（TableRuleBelowWithSeparator）が拒否する")
    },
    (false, None) => TableBoundary::partial_rules(
      frame.rule_gap,
      frame.partial_rule_thickness,
      frame.rule_gap,
      &bands,
      frame.partial_rule_trim,
      frame.rule_color,
    ),
    (true, Some(RowSeparator::Rule)) => separator.clone(),
    (true, Some(RowSeparator::Gap)) => TableBoundary {
      height: frame.row_gap,
      rules: Vec::new(),
    },
    (true, None) => TableBoundary::default(),
  };
}

/// アイテム列の自然幅を返す
fn measure_items_width(items: &[HItem]) -> Length { return items.iter().map(HItem::natural_width).sum(); }

/// `total`（非負）を `parts`（1 以上）個へ sp 単位で等分し、余りを左の部分から 1sp ずつ配る
///
/// `parts == 0` は呼び出し側が避ける（配り先の列が無いときは呼ばない）。
fn split_evenly(total: Length, parts: usize) -> Vec<Length> {
  let parts = i64::try_from(parts).expect("等分の数は列数（Vec の長さで isize::MAX 以下）以下なので i64 に収まる");
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
/// 5. `Auto` 以外を跨ぐ結合セルは、跨ぐ列の幅の合計が最小内容幅 + 左右 padding に足りなければ、不足分を跨ぐ
///    `Flex` 列へ、無ければ跨ぐ `Auto` 列へ均等に足す（`Fixed` / `Ratio` は書き換えず、はみ出しは配置が
///    warning にする）
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

/// 列揃えを、折り返すセルの行分割器へ渡す揃えにする（セルは両端揃えしない）
fn text_alignment(align: ColumnAlign) -> TextAlignment {
  return match align {
    ColumnAlign::Left => TextAlignment::Left,
    ColumnAlign::Center => TextAlignment::Center,
    ColumnAlign::Right => TextAlignment::Right,
  };
}

/// 折り返さないセルの内容を、x = 0 から自然幅で並べた 1 行にする
///
/// 呼び出し側は揃えの位置を [`measure_items_width`]（全アイテムの自然幅の和）から決めるので、行頭・行末の glue も
/// 落とさず `Kern` / `Glue` / `MathBreak` の自然幅ぶん進める。高さ・深さは描く箱（`HItem::Box`）の最大で、
/// `FlushRight` は描かない。対応の無い `LinkEnd` は読み飛ばし、閉じない `LinkStart` は矩形を作らない。
fn unwrapped_line(items: &[HItem]) -> Line {
  let mut line = Line {
    boxes: Vec::new(),
    height: Length::ZERO,
    depth: Length::ZERO,
    links: Vec::new(),
    footnotes: Vec::new(),
    index_marks: Vec::new(),
  };
  let mut cursor = Length::ZERO;
  let mut open: Vec<(LinkTarget, Length)> = Vec::new();
  for item in items {
    match item {
      HItem::Box(hbox) => {
        line.boxes.push(PlacedHBox {
          hbox: hbox.clone(),
          dx: cursor,
          dy: Length::ZERO,
        });
        line.height = line.height.max(hbox.height);
        line.depth = line.depth.max(hbox.depth);
        cursor += hbox.width;
      },
      HItem::Kern(value) => cursor += *value,
      HItem::MathBreak { spacing, .. } => cursor += *spacing,
      HItem::Glue { natural, .. } => cursor += *natural,
      HItem::LinkStart(target) => open.push((target.clone(), cursor)),
      HItem::LinkEnd => {
        if let Some((target, x0)) = open.pop() {
          line.links.push(LineLink {
            target,
            x0,
            x1: cursor,
          });
        }
      },
      HItem::IndexMark(term) => line.index_marks.push(term.clone()),
      HItem::Footnote(footnote) => line.footnotes.push(footnote.clone()),
      HItem::Penalty { .. } | HItem::Discretionary { .. } | HItem::ForcedBreak | HItem::FlushRight(_) => {},
    }
  }
  return line;
}

/// 1 行の各セルを行に組み、表左端を原点に置いた行の列をセル順に返す
///
/// 跨ぐ列のどれかが `Auto` 以外のセルは、帯幅から左右の余白を除いた内容幅（0 で下止め）で `breaker` が開始列の
/// 揃えで折り返し、行頭は帯左端 + 余白。全列 `Auto` のセルは [`unwrapped_line`] の 1 行を、開始列の揃えと余白で
/// 帯の中に置く。
///
/// 戻り値の 2 つ目は、折り返すセルの行が内容幅（帯幅 − 左右の余白。0 で下止めしない）を超えた量の最大で、
/// 折り返さないセルは寄与しない。内容幅が負なら空のセルでも正になる。収まれば 0。
fn layout_row_cells(
  row: &MeasuredRow,
  frame: &TableFrame<'_>,
  col_widths: &[Length],
  breaker: &dyn LineBreaker,
) -> (Vec<Vec<Line>>, Length) {
  let mut overrun = Length::ZERO;
  let mut cells = Vec::with_capacity(row.cells.len());
  for (cell, spanned, cell_x, band_width) in cell_bands(&row.cells, col_widths) {
    let spanned_columns = &frame.columns[spanned];
    let align = spanned_columns.first().map_or(ColumnAlign::Left, |column| return column.align);
    let lines = if wraps(spanned_columns) {
      let content_width = band_width - frame.cell_padding * 2;
      let mut lines = breaker.break_lines(&cell.items, content_width.max(Length::ZERO), text_alignment(align));
      let widest = lines.iter().map(Line::width).fold(Length::ZERO, Length::max);
      overrun = overrun.max(widest - content_width);
      for line in &mut lines {
        line.shift_x(cell_x + frame.cell_padding);
      }
      lines
    } else {
      let content_width = measure_items_width(&cell.items);
      let content_x = match align {
        ColumnAlign::Left => cell_x + frame.cell_padding,
        ColumnAlign::Center => cell_x + (band_width - content_width) / 2.0,
        ColumnAlign::Right => cell_x + band_width - frame.cell_padding - content_width,
      };
      let mut line = unwrapped_line(&cell.items);
      line.shift_x(content_x);
      vec![line]
    };
    cells.push(lines);
  }
  return (cells, overrun);
}

/// セルごとの帯 — セル・跨ぐ列の範囲・表左端からの帯の左端 x・帯幅（跨ぐ列幅の和）
///
/// 跨ぐ列数は残りの列数で切り詰める（frontend が全行の `column_span` の和を列数に揃えるので、実入力では切り詰めない）。
fn cell_bands<'r>(
  cells: &'r [MeasuredCell],
  col_widths: &[Length],
) -> Vec<(&'r MeasuredCell, Range<usize>, Length, Length)> {
  let mut column_index = 0usize;
  let mut cell_x = Length::ZERO;
  let mut bands = Vec::with_capacity(cells.len());
  for cell in cells {
    let column_span = (cell.column_span as usize).min(col_widths.len().saturating_sub(column_index));
    let spanned = column_index..column_index + column_span;
    let band_width: Length = col_widths[spanned.clone()].iter().copied().sum();
    bands.push((cell, spanned, cell_x, band_width));
    cell_x += band_width;
    column_index += column_span;
  }
  return bands;
}

/// 1 行を、表左端・行帯上端を原点とする行断片にする
///
/// 行送りを ℓ とする。先頭行帯は全セルの先頭行をまとめて `band₀ = max(ℓ, h₀ + d₀)`（h₀ / d₀ は全セルの先頭行の
/// 高さ・深さの最大）とし、余りを上下へ等分した `B = h₀ + (band₀ − h₀ − d₀) / 2` を全セルの先頭行が共有する
/// ベースライン（`baseline_y`）にする。各セルの続き行 k は行ごとの帯 `bandₖ = max(ℓ, hₖ + dₖ)` を下へ積み、
/// ベースラインは帯の中で同じく上下を等分した位置。行の高さは `band₀` に、続き行の帯の和が最大のセルのその和を
/// 足したもので、描く箱はどれも自分の行の帯に収まる。続き行の箱は `dy` からベースラインの差を引く（描画は
/// `baseline_y − dy`）。リンク矩形の縦範囲はリンクが乗る行の帯で、索引語・脚注はセル順 → 行順に移す。
///
/// `row_span > 1` のセル（結合セル）も先頭行は先頭行帯とベースラインに参加し、続き行を同じく下へ積むが、行の高さには
/// 足さない — 戻り値の 2 つ目に、結合セルごとの（結合行数, 内容の高さ = 先頭行帯 + 続き行の帯の和）をセル順に返し、
/// 結合した行の高さとの差は [`merge_row_group`] が結合の最後の行に足す。
fn row_fragment(
  row: &MeasuredRow,
  frame: &TableFrame<'_>,
  col_widths: &[Length],
  breaker: &dyn LineBreaker,
) -> (TableRowFragment, Vec<(u32, Length)>) {
  let (cells, overrun) = layout_row_cells(row, frame, col_widths, breaker);
  let (ascent, descent) = cells.iter().filter_map(|lines| return lines.first()).fold(
    (Length::ZERO, Length::ZERO),
    |(ascent, descent), line| {
      return (ascent.max(line.height), descent.max(line.depth));
    },
  );
  let first_band = row.leading.max(ascent + descent);
  let baseline = ascent + (first_band - ascent - descent) / 2.0;
  let mut fragment = TableRowFragment {
    origin: row.origin,
    overrun,
    rules: Vec::new(),
    above: TableBoundary::default(),
    row: PlacedTableRow {
      top_y: Length::ZERO,
      height: first_band,
      baseline_y: baseline,
      boxes: Vec::new(),
    },
    links: Vec::new(),
    index_marks: Vec::new(),
    footnotes: Vec::new(),
  };
  let mut merged = Vec::new();
  for (cell, lines) in row.cells.iter().zip(cells) {
    let mut lines = lines.into_iter();
    if let Some(first) = lines.next() {
      take_line(&mut fragment, first, Length::ZERO, first_band, Length::ZERO);
    }
    let mut band_top = first_band;
    for line in lines {
      let band = row.leading.max(line.height + line.depth);
      let line_baseline = band_top + line.height + (band - line.height - line.depth) / 2.0;
      take_line(&mut fragment, line, band_top, band, line_baseline - baseline);
      band_top += band;
    }
    if cell.row_span > 1 {
      merged.push((cell.row_span, band_top));
    } else {
      fragment.row.height = fragment.row.height.max(band_top);
    }
  }
  return (fragment, merged);
}

/// 行群 `rows` を行の集まり（行結合でつながる行の極大の連続範囲）に分ける
///
/// 行 j のセルが `j + row_span > k + 1` なら境界 k（行 k と k + 1 の間）をまたぎ、またぐセルのある境界の上下の行は
/// 同じ集まりに入る。境界の判定に列は要らない。結合の到達は `rows.len()` で切り詰める（frontend が結合を行群の中に
/// 収める（`TableRowSpanOverflow`）ので、実入力では切り詰めない）。
fn row_groups(rows: &[MeasuredRow]) -> Vec<Range<usize>> {
  let mut groups = Vec::new();
  let mut start = 0usize;
  let mut reach = 0usize;
  for (index, row) in rows.iter().enumerate() {
    if index > start && index >= reach {
      groups.push(start..index);
      start = index;
    }
    let span = row.cells.iter().map(|cell| return cell.row_span as usize).max().unwrap_or(1);
    reach = reach.max((index + span).min(rows.len()));
  }
  if start < rows.len() {
    groups.push(start..rows.len());
  }
  return groups;
}

/// 行の集まりの各行の断片 `parts`（[`row_fragment`] の戻りに上の境界を入れたもの。`parts[k].0.above` は k ≥ 1 で
/// 集まりの内側の境界）を 1 つの断片に畳む
///
/// 行 k の行帯を `heights[k]`、上端を `top(k) = Σ_{m<k} heights[m] + Σ_{1≤m≤k} above[m]` とする。結合セルは結合の
/// 最後の行 `e`（集まりの最後の行で切り詰める）の昇順に、内容の高さが領域 `top(e) + heights[e] − top(k)` を超えれば
/// 差を `heights[e]` に足す — 同じ行で終わる結合セルは後のものが伸びた後の領域を見るので、伸びは和でなく最大になる。
///
/// 畳んだ断片は `parts[0]` の `origin` / `above` / ベースラインを持ち、高さは `top(last) + heights[last]`（行帯の和 +
/// 内側の境界の高さの和 + 伸ばした分。`TableBox::height` と配置の収まり判定はこの値をそのまま使う）。行 k の箱は
/// ベースラインの差 `top(k) + baseline_k − baseline_0` を `dy` から引き、リンクは `top(k)` 下げ、索引語・脚注は行順に
/// つなぎ、内側の境界の罫線を `rules` に上から置く。結合セルの無い 1 行は `parts[0]` をそのまま返す。
fn merge_row_group(mut parts: Vec<(TableRowFragment, Vec<(u32, Length)>)>) -> TableRowFragment {
  if parts.len() == 1 && parts[0].1.is_empty() {
    return parts.swap_remove(0).0;
  }
  let last = parts.len() - 1;
  let mut heights: Vec<Length> = parts.iter().map(|(fragment, _)| return fragment.height()).collect();
  let aboves: Vec<Length> = parts.iter().skip(1).map(|(fragment, _)| return fragment.above.height).collect();
  let top = |heights: &[Length], k: usize| -> Length {
    return heights[..k].iter().copied().sum::<Length>() + aboves[..k].iter().copied().sum::<Length>();
  };
  let mut ends: Vec<(usize, usize, Length)> = parts
    .iter()
    .enumerate()
    .flat_map(|(k, (_, merged))| {
      return merged
        .iter()
        .map(move |&(row_span, content)| return (k, (k + row_span as usize - 1).min(last), content));
    })
    .collect();
  ends.sort_by_key(|&(_, end, _)| return end);
  for (k, end, content) in ends {
    let region = top(&heights, end) + heights[end] - top(&heights, k);
    if content > region {
      heights[end] += content - region;
    }
  }

  let mut parts = parts.into_iter().enumerate();
  let (_, (mut merged, _)) = parts.next().expect("row_groups の範囲は空でないので、行の集まりは 1 行以上");
  let baseline = merged.row.baseline_y;
  for (k, (fragment, _)) in parts {
    let top_k = top(&heights, k);
    merged.overrun = merged.overrun.max(fragment.overrun);
    merged.rules.extend(fragment.above.place(Length::ZERO, top_k - fragment.above.height));
    let lowered = top_k + fragment.row.baseline_y - baseline;
    merged.row.boxes.extend(fragment.row.boxes.into_iter().map(|placed| {
      return PlacedHBox {
        dy: placed.dy - lowered,
        ..placed
      };
    }));
    merged.links.extend(fragment.links.into_iter().map(|link| {
      return PlacedLink {
        y: link.y + top_k,
        ..link
      };
    }));
    merged.index_marks.extend(fragment.index_marks);
    merged.footnotes.extend(fragment.footnotes);
  }
  merged.row.height = top(&heights, last) + heights[last];
  return merged;
}

/// セルの 1 行を行断片へ移す。行の帯は行帯上端から `band_top`・高さ `band`、ベースラインは共有ベースラインより
/// `lowered` 下
fn take_line(fragment: &mut TableRowFragment, line: Line, band_top: Length, band: Length, lowered: Length) {
  fragment.row.boxes.extend(line.boxes.into_iter().map(|placed| {
    return PlacedHBox {
      dy: placed.dy - lowered,
      ..placed
    };
  }));
  fragment
    .links
    .extend(line.links.iter().filter_map(|link| return link.place(Length::ZERO, band_top, band)));
  fragment.index_marks.extend(line.index_marks);
  fragment.footnotes.extend(line.footnotes);
}

#[cfg(test)]
mod tests {
  use std::iter;

  use super::{
    MeasuredCell, MeasuredRow, TableFrame, arrange, measure_items_width, resolve_column_widths, row_fragment,
    split_evenly,
  };
  use crate::{
    document::{ColumnAlign, ColumnWidth, NodeId, RowSeparator},
    length::Length,
    project::FontType,
    publication::GlyphRun,
    semantics::LabelId,
    source::SourceId,
    typeset::{
      boxes::{
        AnchorId, FootnoteId, HBox, HBoxContent, HItem, IndexTerm, LinkTarget, MeasuredFootnote, TableRowFragment,
      },
      breaking::KnuthPlassBreaker,
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

  /// 幅 `width`・高さ 8・深さ 2 の、テキストを含まないボックスを作る（既定フォント 10・係数 1 の行送り 10 に
  /// ちょうど収まる）
  fn word_box(width: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// 揃え `align`・幅指定 `Fixed(width)`（pt）の列を作る
  fn fixed_aligned(width: f32, align: ColumnAlign) -> TableColumn {
    return TableColumn {
      align,
      width: ColumnWidth::Fixed(pt(width)),
    };
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
      row_span: 1,
      rule_below: false,
    };
  }

  /// 列方向に `span` 列を結合し、下に部分罫を引くセルを作る
  fn rule_below_cell(items: Vec<HItem>, span: u32) -> MeasuredCell {
    return MeasuredCell {
      rule_below: true,
      ..span_cell(items, span)
    };
  }

  /// 幅 10 のテキスト 1 つのセルを `count` 個並べた行
  fn plain_row(count: usize) -> MeasuredRow {
    return row(iter::repeat_with(|| return cell(vec![glyph_box(10.0, 10.0)])).take(count).collect());
  }

  /// テストの行（[`row`]）が帰属する `\row` の ID
  fn origin() -> NodeId { return NodeId::for_test(SourceId::new(0), 1); }

  /// 区切りの無い行を作る（既定フォントサイズ 10pt・行高係数 1）
  fn row(cells: Vec<MeasuredCell>) -> MeasuredRow { return MeasuredRow::new(origin(), cells, None, pt(10.0), 1.0); }

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
      row_span: 1,
      rule_below: false,
    };
  }

  /// 行方向に `row_span` 行を結合したセルを作る
  fn merged_cell(items: Vec<HItem>, row_span: u32) -> MeasuredCell {
    return MeasuredCell {
      row_span,
      ..cell(items)
    };
  }

  /// 行結合に覆われる位置の空のセルを作る
  fn blank() -> MeasuredCell { return cell(Vec::new()); }

  /// 幅 30 の列で 1 行 1 語に折り返す 3 語（行帯 10 × 3 行）
  fn three_lines() -> Vec<HItem> {
    return vec![
      word_box(25.0),
      space(),
      word_box(25.0),
      space(),
      word_box(25.0),
    ];
  }

  /// 幅を sp の列で返す
  fn sps(widths: &[Length]) -> Vec<i64> { return widths.iter().map(|width| return width.sp()).collect(); }

  /// セル余白 `padding`・罫線なし（太さ 0・空き 0・黒）の表の見た目を作る
  fn frame(columns: &[TableColumn], padding: f32) -> TableFrame<'_> {
    return TableFrame {
      columns,
      cell_padding: pt(padding),
      outer_rule_thickness: Length::ZERO,
      rule_thickness: Length::ZERO,
      rule_gap: Length::ZERO,
      row_gap: Length::ZERO,
      partial_rule_thickness: Length::ZERO,
      partial_rule_trim: Length::ZERO,
      rule_color: None,
    };
  }

  /// booktabs の既定（枠罫 0.8・細罫 0.5・罫線の空き 2・行群の空き 5・部分罫 0.3・詰め 5）に色 #090909 を付けた、
  /// セル余白 0 の表の見た目を作る
  fn ruled_frame(columns: &[TableColumn]) -> TableFrame<'_> {
    return TableFrame {
      outer_rule_thickness: pt(0.8),
      rule_thickness: pt(0.5),
      rule_gap: pt(2.0),
      row_gap: pt(5.0),
      partial_rule_thickness: pt(0.3),
      partial_rule_trim: pt(5.0),
      rule_color: Some([9, 9, 9]),
      ..frame(columns, 0.0)
    };
  }

  /// 上に区切りの罫線を持つ行を作る（既定フォントサイズ 10pt・行高係数 1）
  fn ruled_row(cells: Vec<MeasuredCell>) -> MeasuredRow {
    return MeasuredRow::new(origin(), cells, Some(RowSeparator::Rule), pt(10.0), 1.0);
  }

  /// 幅 10 のテキスト 1 つのセルだけの行
  fn one_cell_row() -> MeasuredRow { return row(vec![cell(vec![glyph_box(10.0, 10.0)])]); }

  /// 幅 10 のテキスト 1 つのセルだけの、上に区切りの罫線を持つ行
  fn one_cell_ruled_row() -> MeasuredRow { return ruled_row(vec![cell(vec![glyph_box(10.0, 10.0)])]); }

  #[test]
  fn arrange_frames_the_table_with_outer_and_head_rules() {
    let columns = [auto(ColumnAlign::Left)];

    let table = arrange(
      origin(),
      &ruled_frame(&columns),
      &[one_cell_row()],
      &[one_cell_row(), one_cell_ruled_row()],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    assert!(close(table.top.height, 2.8), "{:?}", table.top);
    let top = table.top.rules.first().copied().expect("上端の枠罫");
    assert!(close(top.y, 0.0) && close(top.height, 0.8) && top.width == table.width, "{top:?}");
    assert_eq!(top.color, Some([9, 9, 9]));
    assert!(close(table.bottom.height, 2.8), "{:?}", table.bottom);
    let bottom = table.bottom.rules.first().copied().expect("下端の枠罫");
    assert!(close(bottom.y, 2.0) && close(bottom.height, 0.8), "{bottom:?}");
    assert!(close(table.head_bottom.height, 4.5), "{:?}", table.head_bottom);
    assert_eq!(table.head[0].above.height, Length::ZERO);
    assert!(
      close(table.rows[0].above.height, 4.5) && !table.rows[0].above.rules.is_empty(),
      "{:?}",
      table.rows[0].above
    );
    let separator = table.rows[1].above.rules.first().copied().expect("区切りの行の上の罫線");
    assert!(close(separator.y, 2.0) && close(separator.height, 0.5), "{separator:?}");
    assert_eq!(separator.color, Some([9, 9, 9]));
  }

  #[test]
  fn gap_separator_takes_row_gap_without_a_rule() {
    // rule_thickness 0.5・row_gap 5: ヘッダなしの表の 2 行目が gap
    let columns = [auto(ColumnAlign::Left)];
    let gap_row =
      MeasuredRow::new(origin(), vec![cell(vec![glyph_box(10.0, 10.0)])], Some(RowSeparator::Gap), pt(10.0), 1.0);

    let table = arrange(
      origin(),
      &ruled_frame(&columns),
      &[],
      &[one_cell_row(), gap_row],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    assert!(close(table.rows[1].above.height, 5.0), "{:?}", table.rows[1].above);
    assert!(table.rows[1].above.rules.is_empty());
  }

  #[test]
  fn rule_separator_takes_the_rule_and_its_gaps() {
    let columns = [auto(ColumnAlign::Left)];

    let table = arrange(
      origin(),
      &ruled_frame(&columns),
      &[],
      &[one_cell_row(), one_cell_ruled_row()],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    assert!(close(table.rows[1].above.height, 4.5), "{:?}", table.rows[1].above);
    assert!(!table.rows[1].above.rules.is_empty());
  }

  #[test]
  fn first_rows_take_their_boundary_from_the_structure() {
    // ヘッダなしの表: 先頭行が区切りを持っていても上の境界は無く、ヘッダ下の罫線も無い
    let columns = [auto(ColumnAlign::Left)];

    let table = arrange(
      origin(),
      &ruled_frame(&columns),
      &[],
      &[one_cell_ruled_row(), one_cell_row()],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    assert_eq!(table.head_bottom.height, Length::ZERO);
    assert_eq!(table.rows[0].above.height, Length::ZERO);
    assert!(table.rows[0].above.rules.is_empty());
  }

  #[test]
  fn zero_thickness_head_rule_takes_no_space() {
    // rule_thickness = 0・rule_gap = 2: ヘッダ下にも区切りの行の上にも罫線も空きも無い
    let columns = [auto(ColumnAlign::Left)];
    let frame = TableFrame {
      rule_thickness: Length::ZERO,
      ..ruled_frame(&columns)
    };

    let table = arrange(
      origin(),
      &frame,
      &[one_cell_row()],
      &[one_cell_row(), one_cell_ruled_row()],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    assert_eq!(table.head_bottom.height, Length::ZERO);
    assert_eq!(table.rows[0].above.height, Length::ZERO);
    assert_eq!(table.rows[1].above.height, Length::ZERO);
    assert!(table.rows[1].above.rules.is_empty());
  }

  #[test]
  fn zero_outer_thickness_drops_the_frames() {
    let columns = [auto(ColumnAlign::Left)];
    let frame = TableFrame {
      outer_rule_thickness: Length::ZERO,
      ..ruled_frame(&columns)
    };

    let table = arrange(origin(), &frame, &[], &[one_cell_row()], pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.top.height, Length::ZERO);
    assert!(table.top.rules.is_empty());
    assert_eq!(table.bottom.height, Length::ZERO);
    assert!(table.bottom.rules.is_empty());
  }

  #[test]
  fn rule_below_puts_trimmed_partial_rules_above_the_next_row() {
    // 行 0: [span=2 rule_below][rule_below] → 行 1 の上の境界は 2 + 0.3 + 2、罫線 x=5 w=50 と x=65 w=20 が同じ y=2
    let columns = [fixed(30.0), fixed(30.0), fixed(30.0)];
    let first = row(vec![
      rule_below_cell(vec![glyph_box(10.0, 10.0)], 2),
      rule_below_cell(vec![glyph_box(10.0, 10.0)], 1),
    ]);

    let table =
      arrange(origin(), &ruled_frame(&columns), &[], &[first, plain_row(3)], pt(100.0), true, &KnuthPlassBreaker);

    let above = &table.rows[1].above;
    assert!(close(above.height, 4.3), "{above:?}");
    assert_eq!(above.rules.len(), 2, "{above:?}");
    assert!(close(above.rules[0].x, 5.0) && close(above.rules[0].width, 50.0), "{above:?}");
    assert!(close(above.rules[1].x, 65.0) && close(above.rules[1].width, 20.0), "{above:?}");
    assert!(above.rules.iter().all(|rule| return close(rule.y, 2.0) && close(rule.height, 0.3)));
    assert_eq!(above.rules[0].color, Some([9, 9, 9]));
  }

  #[test]
  fn partial_rule_is_trimmed_at_the_outer_edges_too() {
    // 全 3 列を跨ぐ 1 セル → 罫線 1 本 x=5 w=80
    let columns = [fixed(30.0), fixed(30.0), fixed(30.0)];
    let first = row(vec![rule_below_cell(vec![glyph_box(10.0, 10.0)], 3)]);

    let table =
      arrange(origin(), &ruled_frame(&columns), &[], &[first, plain_row(3)], pt(100.0), true, &KnuthPlassBreaker);

    let rules = &table.rows[1].above.rules;
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert!(close(rules[0].x, 5.0) && close(rules[0].width, 80.0), "{rules:?}");
  }

  #[test]
  fn partial_rules_that_trim_away_keep_the_boundary_height() {
    // 詰め 15 は帯幅 30 の半分 — 罫線は残らないが境界は 2 + 0.3 + 2 を占める
    let columns = [fixed(30.0)];
    let frame = TableFrame {
      partial_rule_trim: pt(15.0),
      ..ruled_frame(&columns)
    };
    let first = row(vec![rule_below_cell(vec![glyph_box(10.0, 10.0)], 1)]);

    let table = arrange(origin(), &frame, &[], &[first, plain_row(1)], pt(100.0), true, &KnuthPlassBreaker);

    assert!(close(table.rows[1].above.height, 4.3), "{:?}", table.rows[1].above);
    assert!(table.rows[1].above.rules.is_empty());
  }

  #[test]
  fn zero_partial_rule_thickness_takes_no_space() {
    let columns = [fixed(30.0)];
    let frame = TableFrame {
      partial_rule_thickness: Length::ZERO,
      ..ruled_frame(&columns)
    };
    let first = row(vec![rule_below_cell(vec![glyph_box(10.0, 10.0)], 1)]);

    let table = arrange(origin(), &frame, &[], &[first, plain_row(1)], pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows[1].above.height, Length::ZERO);
    assert!(table.rows[1].above.rules.is_empty());
  }

  #[test]
  fn repeated_head_keeps_the_partial_rules() {
    // 2 段ヘッダの上段の部分罫は再掲ヘッダにも残る
    let columns = [fixed(30.0), fixed(30.0)];
    let upper = row(vec![rule_below_cell(vec![glyph_box(10.0, 10.0)], 2)]);

    let table = arrange(
      origin(),
      &ruled_frame(&columns),
      &[upper, plain_row(2)],
      &[plain_row(2)],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    let above = &table.repeated_head[1].above;
    assert!(close(above.height, 4.3), "{above:?}");
    assert_eq!(above.rules.len(), 1, "{above:?}");
    assert!(close(above.rules[0].x, 5.0) && close(above.rules[0].width, 50.0), "{above:?}");
  }

  #[test]
  fn repeated_head_keeps_the_boundaries_between_head_rows() {
    let columns = [auto(ColumnAlign::Left)];

    let table = arrange(
      origin(),
      &ruled_frame(&columns),
      &[one_cell_row(), one_cell_ruled_row()],
      &[one_cell_row()],
      pt(100.0),
      true,
      &KnuthPlassBreaker,
    );

    assert_eq!(table.repeated_head[0].above.height, Length::ZERO);
    assert!(close(table.repeated_head[1].above.height, 4.5), "{:?}", table.repeated_head[1].above);
    assert!(!table.repeated_head[1].above.rules.is_empty());
  }

  #[test]
  fn row_span_merges_its_rows_into_one_fragment() {
    let columns = [fixed(30.0), fixed(30.0)];
    let rows = [
      row(vec![
        merged_cell(vec![word_box(10.0)], 2),
        cell(vec![word_box(10.0)]),
      ]),
      row(vec![blank(), cell(vec![word_box(10.0)])]),
    ];

    let table = arrange(origin(), &frame(&columns, 0.0), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 1);
    assert!(close(table.rows[0].height(), 20.0));
    // 2 行目のセルは 1 行目のベースラインより 10 下
    assert!(close(table.rows[0].row.boxes[2].dy, -10.0), "{:?}", table.rows[0].row.boxes);
  }

  #[test]
  fn merged_cell_does_not_grow_its_first_row_and_extends_its_last_row() {
    // 結合セルが 3 行（30）、結合した 2 行は 10 + 10 → 最後の行が 10 伸びて集まりは 30。2 行目は 10 から始まる
    let columns = [fixed(30.0), fixed(30.0)];
    let rows = [
      row(vec![merged_cell(three_lines(), 2), cell(vec![word_box(10.0)])]),
      row(vec![blank(), cell(vec![word_box(10.0)])]),
    ];

    let table = arrange(origin(), &frame(&columns, 0.0), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert!(close(table.rows[0].height(), 30.0));
    let last = table.rows[0].row.boxes.last().expect("2 行目のセルの箱がある");
    assert!(close(last.dy, -10.0), "{last:?}");
  }

  #[test]
  fn extension_takes_the_max_over_cells_ending_at_the_same_row() {
    // 行 0–1 を結合する 2 セル（3 行 = 30 と 4 行 = 40）→ 集まりは 40（和の 50 ではない）
    let columns = [fixed(30.0), fixed(30.0)];
    let four_lines = [three_lines(), vec![space(), word_box(25.0)]].concat();
    let rows = [
      row(vec![merged_cell(three_lines(), 2), merged_cell(four_lines, 2)]),
      row(vec![blank(), blank()]),
    ];

    let table = arrange(origin(), &frame(&columns, 0.0), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 1);
    assert!(close(table.rows[0].height(), 40.0), "{:?}", table.rows[0].height());
  }

  #[test]
  fn chained_row_spans_form_one_group() {
    // A が行 0–1、B が行 1–2 → 断片 1 つ・高さ 30
    let columns = [fixed(30.0), fixed(30.0)];
    let rows = [
      row(vec![
        merged_cell(vec![word_box(10.0)], 2),
        cell(vec![word_box(10.0)]),
      ]),
      row(vec![blank(), merged_cell(vec![word_box(10.0)], 2)]),
      row(vec![cell(vec![word_box(10.0)]), blank()]),
    ];

    let table = arrange(origin(), &frame(&columns, 0.0), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 1);
    assert!(close(table.rows[0].height(), 30.0), "{:?}", table.rows[0].height());
  }

  #[test]
  fn separate_row_spans_form_separate_groups() {
    // 行 0–1 と行 2–3 の 2 つの結合 → 断片 2 つ（高さ 20 ずつ）
    let columns = [fixed(30.0), fixed(30.0)];
    let merged_row = || {
      return row(vec![
        merged_cell(vec![word_box(10.0)], 2),
        cell(vec![word_box(10.0)]),
      ]);
    };
    let covered_row = || return row(vec![blank(), cell(vec![word_box(10.0)])]);
    let rows = [merged_row(), covered_row(), merged_row(), covered_row()];

    let table = arrange(origin(), &frame(&columns, 0.0), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 2);
    assert!(table.rows.iter().all(|fragment| return close(fragment.height(), 20.0)), "{:?}", table.rows);
  }

  #[test]
  fn row_of_only_covered_slots_keeps_the_leading() {
    // [merged 2, merged 2] / [blank, blank] → 断片 1 つ・高さ 20
    let columns = [fixed(30.0), fixed(30.0)];
    let rows = [
      row(vec![
        merged_cell(vec![word_box(10.0)], 2),
        merged_cell(vec![word_box(10.0)], 2),
      ]),
      row(vec![blank(), blank()]),
    ];

    let table = arrange(origin(), &frame(&columns, 0.0), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 1);
    assert!(close(table.rows[0].height(), 20.0), "{:?}", table.rows[0].height());
  }

  #[test]
  fn inner_boundary_keeps_the_partial_rules_beside_the_merge() {
    // 行 0: [merged 2][rule_below] / 行 1: [blank][C] → 断片 1 つ・高さ 10 + 4.3 + 10、内側の罫線 1 本 x=35 w=20 y=12
    let columns = [fixed(30.0), fixed(30.0)];
    let rows = [
      row(vec![
        merged_cell(vec![word_box(10.0)], 2),
        rule_below_cell(vec![word_box(10.0)], 1),
      ]),
      row(vec![blank(), cell(vec![word_box(10.0)])]),
    ];

    let table = arrange(origin(), &ruled_frame(&columns), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 1);
    assert!(close(table.rows[0].height(), 24.3), "{:?}", table.rows[0].height());
    let rules = &table.rows[0].rules;
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert!(close(rules[0].x, 35.0) && close(rules[0].width, 20.0) && close(rules[0].y, 12.0), "{rules:?}");
  }

  #[test]
  fn rule_below_of_a_merged_cell_goes_under_its_last_row() {
    // 行 0: [merged 2 + rule_below][B] / 行 1: [blank][C] / 行 2: [D][E] → 断片 2 つ、rows[1].above は高さ 4.3・
    // 罫線 1 本 x=5 w=20 y=2
    let columns = [fixed(30.0), fixed(30.0)];
    let merged_rule_below = MeasuredCell {
      rule_below: true,
      ..merged_cell(vec![word_box(10.0)], 2)
    };
    let rows = [
      row(vec![merged_rule_below, cell(vec![word_box(10.0)])]),
      row(vec![blank(), cell(vec![word_box(10.0)])]),
      row(vec![cell(vec![word_box(10.0)]), cell(vec![word_box(10.0)])]),
    ];

    let table = arrange(origin(), &ruled_frame(&columns), &[], &rows, pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.rows.len(), 2);
    assert!(table.rows[0].rules.is_empty(), "{:?}", table.rows[0].rules);
    let above = &table.rows[1].above;
    assert!(close(above.height, 4.3), "{above:?}");
    assert_eq!(above.rules.len(), 1, "{above:?}");
    assert!(
      close(above.rules[0].x, 5.0) && close(above.rules[0].width, 20.0) && close(above.rules[0].y, 2.0),
      "{above:?}"
    );
  }

  #[test]
  fn repeated_head_keeps_the_merged_rows() {
    // head = [merged 2][X] / [blank][Y]（[X] に rule_below）→ head も repeated_head も 1 断片、高さと rules が等しい
    let columns = [fixed(30.0), fixed(30.0)];
    let head = [
      row(vec![
        merged_cell(vec![word_box(10.0)], 2),
        rule_below_cell(vec![word_box(10.0)], 1),
      ]),
      row(vec![blank(), cell(vec![word_box(10.0)])]),
    ];

    let table = arrange(origin(), &ruled_frame(&columns), &head, &[plain_row(2)], pt(100.0), true, &KnuthPlassBreaker);

    assert_eq!(table.head.len(), 1);
    assert_eq!(table.repeated_head.len(), 1);
    assert!(close(table.head[0].height(), 24.3), "{:?}", table.head[0].height());
    assert_eq!(table.repeated_head[0].height(), table.head[0].height());
    assert_eq!(table.head[0].rules.len(), 1, "{:?}", table.head[0].rules);
    let geometry = |fragment: &TableRowFragment| -> Vec<(Length, Length, Length, Length)> {
      return fragment.rules.iter().map(|rule| return (rule.x, rule.y, rule.width, rule.height)).collect();
    };
    assert_eq!(geometry(&table.repeated_head[0]), geometry(&table.head[0]));
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
      origin(),
      vec![
        cell(vec![glyph_box(10.0, 12.0)]),
        cell(vec![extent_box(3.0, 2.0)]),
      ],
      None,
      pt(9.0),
      1.5,
    );
    let columns = [auto(ColumnAlign::Left); 2];

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).0.row;

    assert!(close(band.height, 18.0), "{band:?}");
    assert!(close(band.baseline_y, 14.0), "{band:?}");
  }

  #[test]
  fn row_band_grows_to_the_content_extent() {
    // 中身は高さ 14・深さ 7 で計 21、行送り 12 × 1.2 = 14.4 を超えるので行帯は中身に一致する
    let row = MeasuredRow::new(
      origin(),
      vec![
        cell(vec![glyph_box(10.0, 12.0)]),
        cell(vec![extent_box(14.0, 7.0)]),
      ],
      None,
      pt(9.0),
      1.2,
    );
    let columns = [auto(ColumnAlign::Left); 2];

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).0.row;

    assert!(close(band.height, 21.0), "{band:?}");
    assert!(close(band.baseline_y, 14.0), "余りが無いのでベースラインは上端 + 高さ: {band:?}");
  }

  #[test]
  fn row_band_without_text_uses_the_default_font_size() {
    // テキストが無いので行送りは既定 9 × 2 = 18。中身は高さ 1 で、余り 17 を上下へ 8.5 ずつ
    let row = MeasuredRow::new(origin(), vec![cell(vec![text_free_box(5.0)])], None, pt(9.0), 2.0);
    let columns = [auto(ColumnAlign::Left)];

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0)], &KnuthPlassBreaker).0.row;

    assert!(close(band.height, 18.0), "{band:?}");
    assert!(close(band.baseline_y, 9.5), "{band:?}");
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
        span_cell(vec![text_free_box(50.0)], 2),
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
        span_cell(vec![text_free_box(20.0)], 2),
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

    let boxes = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).0.row.boxes;

    assert_eq!(boxes.len(), 2);
    assert!(close(boxes[0].dx, 14.0), "右揃えの先頭 x: {boxes:?}");
    assert!(close(boxes[1].dx, 24.0), "box + kern + glue 後の x: {boxes:?}");
  }

  #[test]
  fn row_fragment_aligns_spanned_cell_within_band() {
    // 揃えは開始列（Center）で決まり、被覆列（Left）は読まない
    let row = row(vec![
      span_cell(vec![text_free_box(10.0)], 2),
      cell(vec![text_free_box(4.0)]),
    ]);
    let columns = [
      auto(ColumnAlign::Center),
      auto(ColumnAlign::Left),
      auto(ColumnAlign::Right),
    ];

    let boxes = row_fragment(&row, &frame(&columns, 2.0), &[pt(20.0), pt(30.0), pt(25.0)], &KnuthPlassBreaker)
      .0
      .row
      .boxes;

    assert_eq!(boxes.len(), 2);
    assert!(close(boxes[0].dx, 20.0), "帯幅 50 の中央: {boxes:?}");
    assert!(close(boxes[1].dx, 69.0), "後続セルは帯の右端 50 から始まる: {boxes:?}");
  }

  #[test]
  fn row_fragment_places_padding_from_the_frame() {
    let columns = [auto(ColumnAlign::Left)];
    let frame = frame(&columns, 3.0);

    let fragment = row_fragment(&one_cell_row(), &frame, &[pt(30.0)], &KnuthPlassBreaker).0;

    assert!(close(fragment.row.top_y, 0.0), "{fragment:?}");
    assert!(close(fragment.row.height, 10.0), "{fragment:?}");
    assert!(close(fragment.row.baseline_y, 10.0), "{fragment:?}");
    assert!(close(fragment.row.boxes[0].dx, 3.0), "表左端 + 余白: {fragment:?}");
  }

  #[test]
  fn row_fragment_collects_index_marks_in_cell_order() {
    let row = row(vec![
      cell(vec![text_free_box(5.0), index_mark("a")]),
      cell(vec![index_mark("b"), text_free_box(5.0)]),
    ]);
    let columns = [auto(ColumnAlign::Left); 2];

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(10.0), pt(10.0)], &KnuthPlassBreaker).0;

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

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(10.0), pt(10.0)], &KnuthPlassBreaker).0;

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

    let fragment = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).0;

    let links = &fragment.links;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x, 2.0) && close(links[0].width, 20.0), "{links:?}");
    assert!(close(links[0].y, 0.0) && links[0].height == fragment.row.height, "{links:?}");
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

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).0.links;

    assert_eq!(links.len(), 2);
    assert_eq!(links[0].target, first);
    assert!(close(links[0].x, 0.0) && close(links[0].width, 5.0), "{links:?}");
    assert_eq!(links[1].target, second);
    assert!(close(links[1].x, 8.0) && close(links[1].width, 5.0), "{links:?}");
  }

  #[test]
  fn row_fragment_links_ignore_unbalanced_link_end() {
    // 対応の無い 2 個目の `LinkEnd` は読み飛ばし、幅 0 の矩形は断片に入れない
    let target = LinkTarget::Internal(AnchorId::Label(LabelId::new("x")));
    let row = row(vec![cell(vec![
      HItem::LinkStart(target),
      HItem::LinkEnd,
      HItem::LinkEnd,
    ])]);
    let columns = [auto(ColumnAlign::Left)];

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).0.links;

    assert!(links.is_empty(), "{links:?}");
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

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).0.links;

    assert!(close(links[0].x, 18.0) && close(links[0].width, 10.0), "{links:?}");
  }

  #[test]
  fn row_fragment_links_in_spanned_cell_follow_band() {
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![span_cell(
      vec![
        HItem::LinkStart(target.clone()),
        text_free_box(10.0),
        HItem::LinkEnd,
      ],
      2,
    )]);
    let columns = [auto(ColumnAlign::Right), auto(ColumnAlign::Left)];

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(20.0), pt(30.0)], &KnuthPlassBreaker).0.links;

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, target);
    assert!(close(links[0].x, 38.0) && close(links[0].width, 10.0), "帯幅 50 の右揃え: {links:?}");
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

    let table = arrange(origin(), &frame(&columns, 2.0), &[head_row], &[body_row], pt(100.0), true, &KnuthPlassBreaker);

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

    let table = arrange(origin(), &frame(&columns, 0.0), &head, &[], pt(100.0), true, &KnuthPlassBreaker);

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

    let table = arrange(origin(), &frame(&columns, 2.0), &[], &rows, pt(200.0), false, &KnuthPlassBreaker);

    assert!(close(table.width, 74.0), "{table:?}");
    assert!(!table.breakable);
  }

  #[test]
  fn row_fragment_reports_the_widest_wrapped_overrun() {
    // 内容幅 30 − 2 × 2 = 26 に分割できない幅 40 の箱が 1 つ
    let row = row(vec![cell(vec![text_free_box(40.0)])]);
    let columns = [fixed(30.0)];

    let fragment = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).0;

    assert!(close(fragment.overrun, 14.0), "{:?}", fragment.overrun);
    assert_eq!(fragment.origin, origin());
  }

  #[test]
  fn row_fragment_reports_overrun_when_padding_eats_the_column() {
    // 内容幅 5 − 4 × 2 = −3 は空のセルでも超過 3 になる
    let row = row(vec![cell(Vec::new())]);
    let columns = [fixed(5.0)];

    let fragment = row_fragment(&row, &frame(&columns, 4.0), &[pt(5.0)], &KnuthPlassBreaker).0;

    assert!(close(fragment.overrun, 3.0), "{:?}", fragment.overrun);
  }

  #[test]
  fn auto_cells_report_no_overrun() {
    let row = row(vec![cell(vec![text_free_box(40.0)])]);
    let columns = [auto(ColumnAlign::Left)];

    let fragment = row_fragment(&row, &frame(&columns, 2.0), &[pt(10.0)], &KnuthPlassBreaker).0;

    assert_eq!(fragment.overrun, Length::ZERO);
  }

  #[test]
  fn fixed_cell_wraps_at_the_content_width() {
    // 内容幅 26 に 10 + 5 + 10 = 25 までが入り、3 個目の箱は 2 行目の行頭（余白 2）へ送られる
    let row = row(vec![cell(vec![
      word_box(10.0),
      space(),
      word_box(10.0),
      space(),
      word_box(10.0),
    ])]);
    let columns = [fixed(30.0)];

    let band = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).0.row;

    assert_eq!(band.boxes.len(), 3, "{band:?}");
    assert!(close(band.boxes[2].dx, 2.0) && close(band.boxes[2].dy, -10.0), "{band:?}");
    assert!(close(band.height, 20.0) && close(band.baseline_y, 8.0), "{band:?}");
  }

  #[test]
  fn auto_cell_does_not_wrap() {
    // 列幅 14（内容幅 10）は語 1 個分しかないので、折り返すなら 2 個目以降の語が続き行へ送られる
    let row = row(vec![cell(vec![
      word_box(10.0),
      space(),
      word_box(10.0),
      space(),
      word_box(10.0),
    ])]);
    let columns = [auto(ColumnAlign::Left)];

    let band = row_fragment(&row, &frame(&columns, 2.0), &[pt(14.0)], &KnuthPlassBreaker).0.row;

    assert!(band.boxes.iter().all(|placed| return placed.dy == Length::ZERO), "{band:?}");
    assert!(close(band.height, 10.0), "{band:?}");
  }

  #[test]
  fn first_baselines_align_and_the_tallest_cell_sets_the_height() {
    // 左は 3 行に折れ、右は高さ 12・深さ 2 の箱 1 個。先頭行帯は max(10, 12 + 2) = 14 でベースライン 12、
    // 左の続き行は行送り 10 ずつ下へ積む
    let row = row(vec![
      cell(vec![
        word_box(20.0),
        space(),
        word_box(20.0),
        space(),
        word_box(20.0),
      ]),
      cell(vec![extent_box(12.0, 2.0)]),
    ]);
    let columns = [fixed(30.0); 2];

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0), pt(30.0)], &KnuthPlassBreaker).0.row;

    assert_eq!(band.boxes.len(), 4, "{band:?}");
    assert!(close(band.baseline_y, 12.0), "{band:?}");
    assert!(close(band.boxes[0].dy, 0.0) && close(band.boxes[3].dy, 0.0), "先頭行は共有ベースライン: {band:?}");
    assert!(close(band.boxes[1].dy, -10.0), "{band:?}");
    assert!(close(band.boxes[2].dy, -20.0), "{band:?}");
    assert!(close(band.height, 34.0), "{band:?}");
  }

  #[test]
  fn wrapped_lines_follow_the_column_alignment() {
    let row = row(vec![cell(vec![
      word_box(10.0),
      space(),
      word_box(10.0),
      space(),
      word_box(20.0),
    ])]);
    let columns = [fixed_aligned(30.0, ColumnAlign::Right)];

    let boxes = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).0.row.boxes;

    assert!(close(boxes[0].dx, 5.0), "1 行目（幅 25）の右揃え: {boxes:?}");
    assert!(close(boxes[2].dx, 10.0), "2 行目（幅 20）の右揃え: {boxes:?}");
  }

  #[test]
  fn span_over_non_auto_columns_wraps_at_the_band_width() {
    // 帯幅 40 − 余白 2 = 内容幅 38 に自然幅 45 は入らない
    let row = row(vec![span_cell(vec![word_box(20.0), space(), word_box(20.0)], 2)]);
    let columns = [fixed(20.0), fixed(20.0)];

    let boxes = row_fragment(&row, &frame(&columns, 1.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).0.row.boxes;

    assert_eq!(boxes.len(), 2, "{boxes:?}");
    assert!(close(boxes[1].dx, 1.0) && close(boxes[1].dy, -10.0), "{boxes:?}");
  }

  #[test]
  fn all_auto_span_does_not_wrap() {
    let row = row(vec![span_cell(vec![word_box(20.0), space(), word_box(20.0)], 2)]);
    let columns = [auto(ColumnAlign::Left); 2];

    let boxes = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).0.row.boxes;

    assert!(boxes.iter().all(|placed| return placed.dy == Length::ZERO), "{boxes:?}");
  }

  #[test]
  fn link_rectangles_split_per_wrapped_line() {
    let target = LinkTarget::External("https://example.com".to_string());
    let row = row(vec![cell(vec![
      HItem::LinkStart(target),
      word_box(10.0),
      space(),
      word_box(10.0),
      space(),
      word_box(10.0),
      HItem::LinkEnd,
    ])]);
    let columns = [fixed(30.0)];

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).0.links;

    assert_eq!(links.len(), 2, "{links:?}");
    assert!(close(links[0].x, 0.0) && close(links[0].y, 0.0), "{links:?}");
    assert!(close(links[0].width, 25.0) && close(links[0].height, 10.0), "{links:?}");
    assert!(close(links[1].x, 0.0) && close(links[1].y, 10.0), "{links:?}");
    assert!(close(links[1].width, 10.0) && close(links[1].height, 10.0), "{links:?}");
  }

  #[test]
  fn row_fragment_collects_footnotes_from_continuation_lines() {
    let row = row(vec![cell(vec![
      word_box(10.0),
      space(),
      word_box(10.0),
      space(),
      word_box(10.0),
      footnote(0),
    ])]);
    let columns = [fixed(30.0)];

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).0;

    let indices: Vec<u32> = fragment.footnotes.iter().map(|footnote| return footnote.index).collect();
    assert_eq!(indices, vec![0]);
  }

  #[test]
  fn empty_wrapping_cell_keeps_one_leading_band() {
    let row = row(vec![cell(Vec::new())]);
    let columns = [fixed(30.0)];

    let band = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).0.row;

    assert!(close(band.height, 10.0), "{band:?}");
    assert!(band.boxes.is_empty(), "{band:?}");
  }

  #[test]
  fn zero_content_width_breaks_at_every_opportunity() {
    // 列幅 4 から余白 4 × 2 を引いた内容幅は 0 で止まり、分割機会ごとに折り返す
    let row = row(vec![cell(vec![word_box(3.0), space(), word_box(3.0)])]);
    let columns = [fixed(4.0)];

    let boxes = row_fragment(&row, &frame(&columns, 4.0), &[pt(4.0)], &KnuthPlassBreaker).0.row.boxes;

    assert_eq!(boxes.len(), 2, "{boxes:?}");
    assert!(close(boxes[1].dy, -10.0), "{boxes:?}");
  }
}
