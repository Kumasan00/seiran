//! 表エンジン — 表の組版（セルの組版と計測 → 列幅の決定 → セルの折り返しと行帯 → 行断片 → 再掲ヘッダ）を閉じる
//!
//! 入口は [`Measurer::build_table`]。入力は lowering が列指定・セルのインライン内容・セル余白・罫線の太さと空きと色
//! まで解決した `TableLayout` で、HIR の表語彙は届かない。出力 [`TableBox`] は表左端・行帯上端を原点とする行断片の列と、
//! 罫線と空きを確定した境界（断片の上端・下端の枠罫、ヘッダ下の罫線、各行の上の区切り。規則は `arrange` の doc）で、
//! 配置（`breaking::break_pages`）は段オフセットと揃えを足して置くだけ。
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

use crate::{
  document::{ColumnAlign, ColumnWidth, NodeId, RowSeparator, TextAlignment},
  length::Length,
  typeset::{
    boxes::{HItem, Line, LineLink, LinkTarget, PlacedHBox, PlacedTableRow, TableBoundary, TableBox, TableRowFragment},
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
    return rows
      .iter()
      .enumerate()
      .map(|(index, row)| {
        let above = if index == 0 {
          first_above.clone()
        } else {
          match row.separator {
            Some(RowSeparator::Rule) => separator.clone(),
            Some(RowSeparator::Gap) => TableBoundary {
              height: frame.row_gap,
              rules: Vec::new(),
            },
            None => TableBoundary::default(),
          }
        };
        return TableRowFragment {
          above,
          ..row_fragment(row, frame, &col_widths, breaker)
        };
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
  let mut column_index = 0usize;
  let mut cell_x = Length::ZERO;
  let mut cells = Vec::with_capacity(row.cells.len());
  for cell in &row.cells {
    let column_span = (cell.column_span as usize).min(col_widths.len().saturating_sub(column_index));
    let spanned = column_index..column_index + column_span;
    let band_width: Length = col_widths[spanned.clone()].iter().copied().sum();
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
    cell_x += band_width;
    column_index += column_span;
  }
  return (cells, overrun);
}

/// 1 行を、表左端・行帯上端を原点とする行断片にする
///
/// 行送りを ℓ とする。先頭行帯は全セルの先頭行をまとめて `band₀ = max(ℓ, h₀ + d₀)`（h₀ / d₀ は全セルの先頭行の
/// 高さ・深さの最大）とし、余りを上下へ等分した `B = h₀ + (band₀ − h₀ − d₀) / 2` を全セルの先頭行が共有する
/// ベースライン（`baseline_y`）にする。各セルの続き行 k は行ごとの帯 `bandₖ = max(ℓ, hₖ + dₖ)` を下へ積み、
/// ベースラインは帯の中で同じく上下を等分した位置。行の高さは `band₀` に、続き行の帯の和が最大のセルのその和を
/// 足したもので、描く箱はどれも自分の行の帯に収まる。続き行の箱は `dy` からベースラインの差を引く（描画は
/// `baseline_y − dy`）。リンク矩形の縦範囲はリンクが乗る行の帯で、索引語・脚注はセル順 → 行順に移す。
fn row_fragment(
  row: &MeasuredRow,
  frame: &TableFrame<'_>,
  col_widths: &[Length],
  breaker: &dyn LineBreaker,
) -> TableRowFragment {
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
  for lines in cells {
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
    fragment.row.height = fragment.row.height.max(band_top);
  }
  return fragment;
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
      boxes::{AnchorId, FootnoteId, HBox, HBoxContent, HItem, IndexTerm, LinkTarget, MeasuredFootnote},
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
    };
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
    };
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
      rule_color: None,
    };
  }

  /// booktabs の既定（枠罫 0.8・細罫 0.5・罫線の空き 2・行群の空き 5）に色 #090909 を付けた、セル余白 0 の表の見た目を作る
  fn ruled_frame(columns: &[TableColumn]) -> TableFrame<'_> {
    return TableFrame {
      outer_rule_thickness: pt(0.8),
      rule_thickness: pt(0.5),
      rule_gap: pt(2.0),
      row_gap: pt(5.0),
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

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).row;

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

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).row;

    assert!(close(band.height, 21.0), "{band:?}");
    assert!(close(band.baseline_y, 14.0), "余りが無いのでベースラインは上端 + 高さ: {band:?}");
  }

  #[test]
  fn row_band_without_text_uses_the_default_font_size() {
    // テキストが無いので行送りは既定 9 × 2 = 18。中身は高さ 1 で、余り 17 を上下へ 8.5 ずつ
    let row = MeasuredRow::new(origin(), vec![cell(vec![text_free_box(5.0)])], None, pt(9.0), 2.0);
    let columns = [auto(ColumnAlign::Left)];

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0)], &KnuthPlassBreaker).row;

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

    let boxes = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).row.boxes;

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

    let boxes = row_fragment(&row, &frame(&columns, 2.0), &[pt(20.0), pt(30.0), pt(25.0)], &KnuthPlassBreaker)
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

    let fragment = row_fragment(&one_cell_row(), &frame, &[pt(30.0)], &KnuthPlassBreaker);

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

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(10.0), pt(10.0)], &KnuthPlassBreaker);

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

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(10.0), pt(10.0)], &KnuthPlassBreaker);

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

    let fragment = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker);

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

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).links;

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

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).links;

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

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).links;

    assert!(close(links[0].x, 18.0) && close(links[0].width, 10.0), "{links:?}");
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

    let links = row_fragment(&row, &frame(&columns, 2.0), &[pt(20.0), pt(30.0)], &KnuthPlassBreaker).links;

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

    let fragment = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker);

    assert!(close(fragment.overrun, 14.0), "{:?}", fragment.overrun);
    assert_eq!(fragment.origin, origin());
  }

  #[test]
  fn row_fragment_reports_overrun_when_padding_eats_the_column() {
    // 内容幅 5 − 4 × 2 = −3 は空のセルでも超過 3 になる
    let row = row(vec![cell(Vec::new())]);
    let columns = [fixed(5.0)];

    let fragment = row_fragment(&row, &frame(&columns, 4.0), &[pt(5.0)], &KnuthPlassBreaker);

    assert!(close(fragment.overrun, 3.0), "{:?}", fragment.overrun);
  }

  #[test]
  fn auto_cells_report_no_overrun() {
    let row = row(vec![cell(vec![text_free_box(40.0)])]);
    let columns = [auto(ColumnAlign::Left)];

    let fragment = row_fragment(&row, &frame(&columns, 2.0), &[pt(10.0)], &KnuthPlassBreaker);

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

    let band = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).row;

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

    let band = row_fragment(&row, &frame(&columns, 2.0), &[pt(14.0)], &KnuthPlassBreaker).row;

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

    let band = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0), pt(30.0)], &KnuthPlassBreaker).row;

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

    let boxes = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).row.boxes;

    assert!(close(boxes[0].dx, 5.0), "1 行目（幅 25）の右揃え: {boxes:?}");
    assert!(close(boxes[2].dx, 10.0), "2 行目（幅 20）の右揃え: {boxes:?}");
  }

  #[test]
  fn span_over_non_auto_columns_wraps_at_the_band_width() {
    // 帯幅 40 − 余白 2 = 内容幅 38 に自然幅 45 は入らない
    let row = row(vec![span_cell(vec![word_box(20.0), space(), word_box(20.0)], 2)]);
    let columns = [fixed(20.0), fixed(20.0)];

    let boxes = row_fragment(&row, &frame(&columns, 1.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).row.boxes;

    assert_eq!(boxes.len(), 2, "{boxes:?}");
    assert!(close(boxes[1].dx, 1.0) && close(boxes[1].dy, -10.0), "{boxes:?}");
  }

  #[test]
  fn all_auto_span_does_not_wrap() {
    let row = row(vec![span_cell(vec![word_box(20.0), space(), word_box(20.0)], 2)]);
    let columns = [auto(ColumnAlign::Left); 2];

    let boxes = row_fragment(&row, &frame(&columns, 0.0), &[pt(20.0), pt(20.0)], &KnuthPlassBreaker).row.boxes;

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

    let links = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker).links;

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

    let fragment = row_fragment(&row, &frame(&columns, 0.0), &[pt(30.0)], &KnuthPlassBreaker);

    let indices: Vec<u32> = fragment.footnotes.iter().map(|footnote| return footnote.index).collect();
    assert_eq!(indices, vec![0]);
  }

  #[test]
  fn empty_wrapping_cell_keeps_one_leading_band() {
    let row = row(vec![cell(Vec::new())]);
    let columns = [fixed(30.0)];

    let band = row_fragment(&row, &frame(&columns, 2.0), &[pt(30.0)], &KnuthPlassBreaker).row;

    assert!(close(band.height, 10.0), "{band:?}");
    assert!(band.boxes.is_empty(), "{band:?}");
  }

  #[test]
  fn zero_content_width_breaks_at_every_opportunity() {
    // 列幅 4 から余白 4 × 2 を引いた内容幅は 0 で止まり、分割機会ごとに折り返す
    let row = row(vec![cell(vec![word_box(3.0), space(), word_box(3.0)])]);
    let columns = [fixed(4.0)];

    let boxes = row_fragment(&row, &frame(&columns, 4.0), &[pt(4.0)], &KnuthPlassBreaker).row.boxes;

    assert_eq!(boxes.len(), 2, "{boxes:?}");
    assert!(close(boxes[1].dy, -10.0), "{boxes:?}");
  }
}
