//! ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::Math`）

use crate::{
  document::{MathDelimiter, MathEnvKind},
  length::Length,
  project::FontType,
  typeset::{
    boxes::{Align, Block, HBox, MathRowNumber, PlacedHItem},
    boxing::Measurer,
    lowering::{AtomNode, MathBlockRow},
  },
};

/// セルの列内での水平揃え（環境種別ごとに決まる）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellAlign {
  /// 左揃え
  Left,
  /// 中央揃え
  Center,
  /// 右揃え
  Right,
}

/// 環境種別・行位置・列インデックスから、そのセルの列内での水平揃えを決める
fn cell_align(kind: MathEnvKind, row_idx: usize, n_rows: usize, col: usize) -> CellAlign {
  return match kind {
    MathEnvKind::Align | MathEnvKind::Split => {
      if col.is_multiple_of(2) {
        CellAlign::Right
      } else {
        CellAlign::Left
      }
    },
    MathEnvKind::Multiline => {
      if n_rows <= 1 || (row_idx > 0 && row_idx < n_rows - 1) {
        CellAlign::Center
      } else if row_idx == 0 {
        CellAlign::Left
      } else {
        CellAlign::Right
      }
    },
    MathEnvKind::Gather | MathEnvKind::Matrix { .. } => CellAlign::Center,
    MathEnvKind::Equation | MathEnvKind::Cases => CellAlign::Left,
  };
}

/// 列幅 `col_width` の中に幅 `cell_width` のセルを置くときの水平オフセット（pt）
fn column_offset(align: CellAlign, col_width: Length, cell_width: Length) -> Length {
  return match align {
    CellAlign::Left => Length::ZERO,
    CellAlign::Center => (col_width - cell_width) / 2.0f32,
    CellAlign::Right => col_width - cell_width,
  };
}

/// 環境種別から本体グリッドを囲む左右の区切り括弧グリフ `(左, 右)` を決める
fn delimiter_glyphs(kind: MathEnvKind) -> (Option<&'static str>, Option<&'static str>) {
  return match kind {
    MathEnvKind::Cases => (Some("{"), None),
    MathEnvKind::Matrix { delimiter } => match delimiter {
      MathDelimiter::None => (None, None),
      MathDelimiter::Paren => (Some("("), Some(")")),
      MathDelimiter::Bracket => (Some("["), Some("]")),
      MathDelimiter::Brace => (Some("{"), Some("}")),
      MathDelimiter::Bar => (Some("|"), Some("|")),
      MathDelimiter::DoubleBar => (Some("\u{2016}"), Some("\u{2016}")),
    },
    // 揃え系の環境は括弧で囲まない。
    MathEnvKind::Equation | MathEnvKind::Align | MathEnvKind::Gather | MathEnvKind::Split | MathEnvKind::Multiline => {
      (None, None)
    },
  };
}

/// 行を measure したあとの中間表現
struct MeasuredRow {
  /// セルごとの閉じた Atom
  cells: Vec<HBox>,
  /// 行番号ボックス（採番された行のみ）
  number: Option<HBox>,
}

impl Measurer<'_> {
  /// `LayoutNode::MathBlock` を measure して `Block::Math` に合成する
  #[expect(
    clippy::too_many_arguments,
    reason = "数式ブロック 1 件の合成に要る値を束ねる中間型を作っても、呼び出し側が同じ数の値を詰め替えるだけになる"
  )]
  pub(crate) fn build_math_block(
    &mut self,
    kind: MathEnvKind,
    rows: Vec<MathBlockRow>,
    env_number: Option<Vec<AtomNode>>,
    align: Align,
    numbers_on_right: bool,
    row_gap: Length,
    column_gap: Length,
  ) -> Block {
    let measured: Vec<MeasuredRow> = rows
      .into_iter()
      .map(|row| {
        let cells = row.cells.into_iter().map(|cell| return self.build_atom(Length::ZERO, cell)).collect();
        let number = row.number.map(|number| return self.build_atom(Length::ZERO, number));
        return MeasuredRow { cells, number };
      })
      .collect();

    let n_rows = measured.len();
    let ncols = measured.iter().map(|row| return row.cells.len()).max().unwrap_or(0);
    let mut col_widths = vec![Length::ZERO; ncols];
    for row in &measured {
      for (c, cell) in row.cells.iter().enumerate() {
        col_widths[c] = col_widths[c].max(cell.width);
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
      let row_height = row.cells.iter().map(|cell| return cell.height).fold(Length::ZERO, Length::max);
      let row_depth = row.cells.iter().map(|cell| return cell.depth).fold(Length::ZERO, Length::max);
      if i > 0 {
        baseline_dy -= prev_depth + row_gap + row_height;
      }
      for (c, cell) in row.cells.into_iter().enumerate() {
        let intra = column_offset(cell_align(kind, i, n_rows, c), col_widths[c], cell.width);
        placed.push(PlacedHItem {
          item: cell,
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

    let (left, right) = delimiter_glyphs(kind);
    if left.is_some() || right.is_some() {
      body = self.wrap_with_delimiters(body, left, right);
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
  fn wrap_with_delimiters(&mut self, body: HBox, left: Option<&str>, right: Option<&str>) -> HBox {
    let body_height = body.height;
    let body_depth = body.depth;
    let body_width = body.width;
    let body_center = (body_height - body_depth) / 2.0;
    let gap = self.default_font_size * 0.15;

    let mut children: Vec<PlacedHItem> = Vec::new();
    let mut dx = Length::ZERO;
    if let Some(ch) = left {
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
    if let Some(ch) = right {
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

#[cfg(test)]
mod tests {
  use super::{CellAlign, cell_align, column_offset, delimiter_glyphs};
  use crate::{
    document::{MathDelimiter, MathEnvKind},
    length::Length,
  };

  #[test]
  fn cell_align_align_and_split_alternate_right_left_by_column() {
    for kind in [MathEnvKind::Align, MathEnvKind::Split] {
      assert_eq!(cell_align(kind, 0, 1, 0), CellAlign::Right, "列 0 は右: {kind:?}");
      assert_eq!(cell_align(kind, 0, 1, 1), CellAlign::Left, "列 1 は左: {kind:?}");
      assert_eq!(cell_align(kind, 0, 1, 2), CellAlign::Right, "列 2 は右: {kind:?}");
    }
  }

  #[test]
  fn cell_align_gather_is_always_center() {
    assert_eq!(cell_align(MathEnvKind::Gather, 0, 3, 0), CellAlign::Center);
    assert_eq!(cell_align(MathEnvKind::Gather, 1, 3, 0), CellAlign::Center);
    assert_eq!(cell_align(MathEnvKind::Gather, 2, 3, 0), CellAlign::Center);
  }

  #[test]
  fn cell_align_multiline_is_staircase() {
    let kind = MathEnvKind::Multiline;
    assert_eq!(cell_align(kind, 0, 3, 0), CellAlign::Left, "先頭行は左");
    assert_eq!(cell_align(kind, 1, 3, 0), CellAlign::Center, "中間行は中央");
    assert_eq!(cell_align(kind, 2, 3, 0), CellAlign::Right, "末尾行は右");
  }

  #[test]
  fn cell_align_multiline_single_row_is_center() {
    assert_eq!(cell_align(MathEnvKind::Multiline, 0, 1, 0), CellAlign::Center);
  }

  #[test]
  fn cell_align_matrix_center_equation_and_cases_left() {
    assert_eq!(
      cell_align(
        MathEnvKind::Matrix {
          delimiter: MathDelimiter::None
        },
        0,
        2,
        0
      ),
      CellAlign::Center
    );
    assert_eq!(cell_align(MathEnvKind::Equation, 0, 1, 0), CellAlign::Left);
    assert_eq!(cell_align(MathEnvKind::Cases, 0, 2, 0), CellAlign::Left);
  }

  #[test]
  fn delimiter_glyphs_maps_cases_and_matrix() {
    assert_eq!(delimiter_glyphs(MathEnvKind::Cases), (Some("{"), None));
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::Bracket
      }),
      (Some("["), Some("]"))
    );
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::Paren
      }),
      (Some("("), Some(")"))
    );
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::Brace
      }),
      (Some("{"), Some("}"))
    );
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::Bar
      }),
      (Some("|"), Some("|"))
    );
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::DoubleBar
      }),
      (Some("\u{2016}"), Some("\u{2016}"))
    );
  }

  #[test]
  fn delimiter_glyphs_absent_for_none_and_other_envs() {
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::None
      }),
      (None, None)
    );
    assert_eq!(delimiter_glyphs(MathEnvKind::Equation), (None, None));
    assert_eq!(delimiter_glyphs(MathEnvKind::Align), (None, None));
    assert_eq!(delimiter_glyphs(MathEnvKind::Gather), (None, None));
    assert_eq!(delimiter_glyphs(MathEnvKind::Split), (None, None));
    assert_eq!(delimiter_glyphs(MathEnvKind::Multiline), (None, None));
  }

  #[test]
  fn column_offset_places_cell_within_column_width() {
    assert_eq!(column_offset(CellAlign::Left, Length::pt(10.0), Length::pt(4.0)), Length::ZERO);
    assert_eq!(column_offset(CellAlign::Center, Length::pt(10.0), Length::pt(4.0)), Length::pt(3.0));
    assert_eq!(column_offset(CellAlign::Right, Length::pt(10.0), Length::pt(4.0)), Length::pt(6.0));
  }
}
