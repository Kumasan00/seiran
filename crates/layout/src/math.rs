//! ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::MathBlock`）
//!
//! 各セル（`&` 区切りの列）を閉じた Atom（`HBox`）に measure し、環境種別 [`MathEnvKind`]
//! に応じて列を整列・行を縦積みして「環境全体 = 1 つの本体 Atom」に合成する。数式の列は
//! すべて自然幅なので、列幅・行高・区切り括弧の配置はすべてこの段で局所座標まで確定できる。
//! 本文幅に依存する処理（本体の中央寄せ・番号の端寄せ）だけを `hlist::break_pages` に委ねる。

use hlist::{Block, HBox, MathRowNumber, PlacedHItem};
use lowering::{LayoutNode, MathBlockRow};
use types::{Align, FontType, Length, MathDelimiter, MathEnvKind};

use crate::Measurer;

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
///
/// - `align` / `split`: 奇数列（0 始まりで偶数番目）を右、続く列を左に寄せて `&` 位置で接合する
/// - `gather`: 全セル中央揃え（単一列なのでブロック内中央寄せになる）
/// - `multiline`: 単一列で、先頭行=左・末尾行=右・中間行=中央の階段配置（1 行なら中央）
/// - `matrix`: 全セル中央揃え
/// - `equation` / `cases`: 左揃え
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
    // gather は単一列の中央寄せ、matrix はグリッド中央寄せ（いずれも結果は中央揃え）
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
///
/// `cases` は左の大波括弧のみ（右は無し）、`matrix` は `[delimiter=...]` の指定に応じた左右ペア
/// （`none` は括弧なし）。区切り括弧を持たない環境（`equation` / `align` 等）は `(None, None)`。
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
    _ => (None, None),
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
  /// `LayoutNode::MathBlock` を measure して `Block::MathBlock` に合成する
  ///
  /// 各セルを `build_atom` で閉じた Atom にし、列ごとの最大幅で列位置を確定、行を縦に
  /// 積んで 1 つの本体 Atom にまとめる。行ごと番号（`align` / `gather`）は各行のベースラインに、
  /// 環境全体に 1 つの番号（`split` / `multiline` の `env_number`）はブロック縦中央に別ボックスとして
  /// 保持し、最終的な端寄せ（`numbers_on_right`）は `break_pages` に委ねる。
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn build_math_block(
    &mut self,
    kind: MathEnvKind,
    rows: Vec<MathBlockRow>,
    env_number: Option<Vec<LayoutNode>>,
    align: Align,
    numbers_on_right: bool,
    row_gap: Length,
    column_gap: Length,
  ) -> Block {
    // 各セル・番号を閉じた Atom に measure する
    let measured: Vec<MeasuredRow> = rows
      .into_iter()
      .map(|row| {
        let cells = row.cells.into_iter().map(|cell| self.build_atom(Length::ZERO, cell)).collect();
        let number = row.number.map(|number| self.build_atom(Length::ZERO, number));
        return MeasuredRow { cells, number };
      })
      .collect();

    // 列幅 = その列のセルの最大幅。列位置 = 列幅 + 列間アキの累積
    let n_rows = measured.len();
    let ncols = measured.iter().map(|row| row.cells.len()).max().unwrap_or(0);
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

    // 行を縦に積む。行 0 のベースラインを本体ベースライン（dy = 0）とし、以降は下方向へ。
    let mut placed: Vec<PlacedHItem> = Vec::new();
    let mut numbers: Vec<MathRowNumber> = Vec::new();
    let mut baseline_dy = Length::ZERO;
    let mut prev_depth = Length::ZERO;
    for (i, row) in measured.into_iter().enumerate() {
      let row_height = row.cells.iter().map(|cell| cell.height).fold(Length::ZERO, Length::max);
      let row_depth = row.cells.iter().map(|cell| cell.depth).fold(Length::ZERO, Length::max);
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

    // 環境全体に 1 つの番号（split / multiline）はブロック縦中央に配置する。
    // body のベースライン（dy = 0）基準でブロック中央へ番号の視覚中央を合わせる
    // （dy は正で上方向。`break_pages` が `baseline_y - dy` を番号ベースラインにする）。
    if let Some(env_number) = env_number {
      let number = self.build_atom(Length::ZERO, env_number);
      let center_dy = (body.height - body.depth) / 2.0 - (number.height - number.depth) / 2.0;
      numbers.push(MathRowNumber {
        content: number,
        dy: center_dy,
      });
    }

    // 区切り括弧（cases / matrix）は本体 Atom を左右の括弧で挟んで包み直す。
    // cases / matrix は無採番（`numbers` 空）なので本体の水平シフトは番号配置に影響しない。
    let (left, right) = delimiter_glyphs(kind);
    if left.is_some() || right.is_some() {
      body = self.wrap_with_delimiters(body, left, right);
    }

    return Block::MathBlock {
      body,
      numbers,
      numbers_on_right,
      align,
    };
  }

  /// 区切り括弧グリフを本体グリッドの高さ・深さに合わせて拡大した閉じたボックスにして返す
  ///
  /// `ch` を数式フォントで基準サイズにシェーピングして自然な高さ＋深さを得たのち、目標高さ
  /// （`target_height + target_depth` に上下の余白を足した値）に対する倍率でフォントサイズを拡大して
  /// 再シェーピングする。OpenType MATH 由来の伸縮グリフ組立ではなく、1 グリフの一様拡大で近似する
  /// （read-fonts が MATH 未対応のため。本物の伸縮は将来対応 → #67 周辺）。
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
  ///
  /// 各括弧は本体の縦中央（数式軸 `(height - depth) / 2`）にグリフの縦中央を合わせて配置し、左括弧→
  /// 本体→右括弧を水平に並べて 1 つの Atom にまとめる。右が `None` の場合（`cases`）は左括弧のみ付く。
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
  use types::{Length, MathDelimiter, MathEnvKind};

  use super::{CellAlign, cell_align, column_offset, delimiter_glyphs};

  #[test]
  fn cell_align_align_and_split_alternate_right_left_by_column() {
    // Arrange & Act & Assert — align / split は偶数列(0始まり)=右・奇数列=左で `&` 位置に接合
    for kind in [MathEnvKind::Align, MathEnvKind::Split] {
      assert_eq!(cell_align(kind, 0, 1, 0), CellAlign::Right, "列 0 は右: {kind:?}");
      assert_eq!(cell_align(kind, 0, 1, 1), CellAlign::Left, "列 1 は左: {kind:?}");
      assert_eq!(cell_align(kind, 0, 1, 2), CellAlign::Right, "列 2 は右: {kind:?}");
    }
  }

  #[test]
  fn cell_align_gather_is_always_center() {
    // Arrange & Act & Assert — gather は全行中央（単一列なのでブロック内中央寄せ）
    assert_eq!(cell_align(MathEnvKind::Gather, 0, 3, 0), CellAlign::Center);
    assert_eq!(cell_align(MathEnvKind::Gather, 1, 3, 0), CellAlign::Center);
    assert_eq!(cell_align(MathEnvKind::Gather, 2, 3, 0), CellAlign::Center);
  }

  #[test]
  fn cell_align_multiline_is_staircase() {
    // Arrange & Act & Assert — multiline は先頭=左・末尾=右・中間=中央
    let kind = MathEnvKind::Multiline;
    assert_eq!(cell_align(kind, 0, 3, 0), CellAlign::Left, "先頭行は左");
    assert_eq!(cell_align(kind, 1, 3, 0), CellAlign::Center, "中間行は中央");
    assert_eq!(cell_align(kind, 2, 3, 0), CellAlign::Right, "末尾行は右");
  }

  #[test]
  fn cell_align_multiline_single_row_is_center() {
    // Arrange & Act & Assert — 1 行だけの multiline は中央
    assert_eq!(cell_align(MathEnvKind::Multiline, 0, 1, 0), CellAlign::Center);
  }

  #[test]
  fn cell_align_matrix_center_equation_and_cases_left() {
    // Arrange & Act & Assert — matrix は中央、equation / cases は左
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
    // Arrange & Act & Assert — cases は左波括弧のみ、matrix は delimiter 指定に応じた左右ペア
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
  }

  #[test]
  fn delimiter_glyphs_absent_for_none_and_other_envs() {
    // Arrange & Act & Assert — matrix の delimiter=none と、括弧を持たない環境は (None, None)
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Matrix {
        delimiter: MathDelimiter::None
      }),
      (None, None)
    );
    assert_eq!(delimiter_glyphs(MathEnvKind::Equation), (None, None));
    assert_eq!(delimiter_glyphs(MathEnvKind::Align), (None, None));
  }

  #[test]
  fn column_offset_places_cell_within_column_width() {
    // Arrange — 列幅 10、セル幅 4
    // Act & Assert — 左=0、中央=(10-4)/2=3、右=10-4=6
    assert_eq!(column_offset(CellAlign::Left, Length::pt(10.0), Length::pt(4.0)), Length::ZERO);
    assert_eq!(column_offset(CellAlign::Center, Length::pt(10.0), Length::pt(4.0)), Length::pt(3.0));
    assert_eq!(column_offset(CellAlign::Right, Length::pt(10.0), Length::pt(4.0)), Length::pt(6.0));
  }
}
