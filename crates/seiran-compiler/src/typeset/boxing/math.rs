//! ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::Math`）

use crate::{
  length::Length,
  project::FontType,
  typeset::{
    boxes::{Align, Block, HBox, MathRowNumber, PlacedHItem},
    boxing::Measurer,
    lowering::{DelimiterGlyphs, MathBlockLayout},
  },
};

/// 行を measure したあとの中間表現
struct MeasuredRow {
  /// セルごとの計測結果
  cells: Vec<MeasuredCell>,
  /// 行番号ボックス（採番された行のみ）
  number: Option<HBox>,
}

/// セルを measure したあとの中間表現
struct MeasuredCell {
  /// 閉じた Atom
  content: HBox,
  /// 列内での水平揃え（lowering が解決済み）
  align: Align,
}

impl Measurer<'_> {
  /// `LayoutNode::MathBlock` を measure して `Block::Math` に合成する
  pub(crate) fn build_math_block(&mut self, block: MathBlockLayout) -> Block {
    let MathBlockLayout {
      rows,
      env_number,
      align,
      numbers_on_right,
      row_gap,
      column_gap,
      delimiters,
    } = block;

    let measured: Vec<MeasuredRow> = rows
      .into_iter()
      .map(|row| {
        let cells = row
          .cells
          .into_iter()
          .map(|cell| {
            return MeasuredCell {
              content: self.build_atom(Length::ZERO, cell.content),
              align: cell.align,
            };
          })
          .collect();
        let number = row.number.map(|number| return self.build_atom(Length::ZERO, number));
        return MeasuredRow { cells, number };
      })
      .collect();

    let ncols = measured.iter().map(|row| return row.cells.len()).max().unwrap_or(0);
    let mut col_widths = vec![Length::ZERO; ncols];
    for row in &measured {
      for (c, cell) in row.cells.iter().enumerate() {
        col_widths[c] = col_widths[c].max(cell.content.width);
      }
    }
    let mut col_x = vec![Length::ZERO; ncols];
    let mut acc = Length::ZERO;
    for c in 0..ncols {
      col_x[c] = acc;
      acc += col_widths[c] + column_gap;
    }

    let mut placed: Vec<PlacedHItem> = Vec::new();
    let mut numbers: Vec<MathRowNumber> = Vec::new();
    let mut baseline_dy = Length::ZERO;
    let mut prev_depth = Length::ZERO;
    for (i, row) in measured.into_iter().enumerate() {
      let row_height = row.cells.iter().map(|cell| return cell.content.height).fold(Length::ZERO, Length::max);
      let row_depth = row.cells.iter().map(|cell| return cell.content.depth).fold(Length::ZERO, Length::max);
      if i > 0 {
        baseline_dy -= prev_depth + row_gap + row_height;
      }
      for (c, cell) in row.cells.into_iter().enumerate() {
        // 列幅は列内のセル幅の最大値なので、`Align::offset` の 0 へのクランプは到達しない。
        let intra = cell.align.offset(col_widths[c], cell.content.width);
        placed.push(PlacedHItem {
          item: cell.content,
          dy: baseline_dy,
          dx: col_x[c] + intra,
        });
      }
      if let Some(number) = row.number {
        numbers.push(MathRowNumber {
          content: number,
          dy: baseline_dy,
        });
      }
      prev_depth = row_depth;
    }

    let mut body = HBox::atom(placed);

    // `dy` は上向きなので、番号の視覚中央を本体中央へ合わせる。
    if let Some(env_number) = env_number {
      let number = self.build_atom(Length::ZERO, env_number);
      let center_dy = (body.height - body.depth) / 2.0 - (number.height - number.depth) / 2.0;
      numbers.push(MathRowNumber {
        content: number,
        dy: center_dy,
      });
    }

    if delimiters.is_present() {
      body = self.wrap_with_delimiters(body, delimiters);
    }

    return Block::Math {
      body,
      numbers,
      numbers_on_right,
      align,
    };
  }

  /// 区切り括弧グリフを本体グリッドの高さ・深さに合わせて拡大した閉じたボックスにして返す
  fn shape_delimiter(&mut self, ch: &str, target_height: Length, target_depth: Length) -> HBox {
    let base = self.default_font_size;
    let natural = self.shape_segment(ch, FontType::Math, base, None);
    let natural_total = natural.height + natural.depth;
    let pad = base * 0.1;
    let target_total = target_height + target_depth + pad * 2;
    // 拡大のみ（自然サイズより小さくはしない）。小さなグリッドでも括弧は通常字より縮めない
    let scale = if natural_total.is_positive() {
      target_total.ratio(natural_total).max(1.0)
    } else {
      1.0
    };
    return self.shape_segment(ch, FontType::Math, base.scale(scale), None);
  }

  /// 本体 Atom を左右の区切り括弧で挟んで包み直す
  fn wrap_with_delimiters(&mut self, body: HBox, delimiters: DelimiterGlyphs) -> HBox {
    let body_height = body.height;
    let body_depth = body.depth;
    let body_width = body.width;
    let body_center = (body_height - body_depth) / 2.0;
    let gap = self.default_font_size * 0.15;

    let mut children: Vec<PlacedHItem> = Vec::new();
    let mut dx = Length::ZERO;
    if let Some(ch) = delimiters.left {
      let delim = self.shape_delimiter(ch, body_height, body_depth);
      let dy = body_center - (delim.height - delim.depth) / 2.0;
      let width = delim.width;
      children.push(PlacedHItem {
        item: delim,
        dy,
        dx,
      });
      dx += width + gap;
    }
    children.push(PlacedHItem {
      item: body,
      dy: Length::ZERO,
      dx,
    });
    dx += body_width + gap;
    if let Some(ch) = delimiters.right {
      let delim = self.shape_delimiter(ch, body_height, body_depth);
      let dy = body_center - (delim.height - delim.depth) / 2.0;
      children.push(PlacedHItem {
        item: delim,
        dy,
        dx,
      });
    }
    return HBox::atom(children);
  }
}
