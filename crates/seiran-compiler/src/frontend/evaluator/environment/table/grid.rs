//! 表の行群の格子 — 行結合が覆う列の追跡と、行結合・区切り・部分罫の境界の検査

use std::ops::Range;

use crate::{
  document::{HirTableCell, HirTableRow, NodeId},
  frontend::evaluator::{EvalContext, EvalError},
};

/// 行結合（`\cell[rowspan=N]`）が下の行で覆う列の追跡
///
/// 1 つの行群（`\head` か本体）につき 1 つ持ち、行を評価し終えるたびに [`Coverage::advance`] を呼ぶ。
#[derive(Default)]
pub(super) struct Coverage {
  /// 列ごとの「この先まだ覆う行数」と結合セルの ID（覆われていない列は `None`）
  columns: Vec<Option<(u32, NodeId)>>,
}

impl Coverage {
  /// 列範囲の中で最初に覆われている列の結合セルを返す
  pub(super) fn owner(&self, columns: Range<usize>) -> Option<NodeId> {
    return columns
      .filter_map(|column| return self.columns.get(column).copied().flatten())
      .map(|(_, owner)| return owner)
      .next();
  }

  /// 1 行を評価し終えたときに呼び、残りの行数を 1 減らして、その行の結合セルを登録する
  pub(super) fn advance(&mut self, cells: &[HirTableCell]) {
    for slot in &mut self.columns {
      *slot = match *slot {
        Some((remaining, owner)) if remaining > 1 => Some((remaining - 1, owner)),
        Some(_) | None => None,
      };
    }
    let mut column = 0usize;
    for cell in cells {
      let end = column + cell.column_span as usize;
      if cell.row_span > 1 {
        if self.columns.len() < end {
          self.columns.resize(end, None);
        }
        for slot in &mut self.columns[column..end] {
          *slot = Some((cell.row_span - 1, cell.id));
        }
      }
      column = end;
    }
  }
}

/// 行群（`\head` か本体）の行結合・部分罫の境界を検査する
///
/// 結合は行群の最後の行を超えられない。`\head` と本体は別の行群なので、またぐ結合もここで拒否される。
/// `rule_below` は行群の最後の行の下（枠罫の持ち場）と、次の行が `separator` を持つ境界に置けない。
pub(super) fn check_row_group(group: &[HirTableRow], ctx: &EvalContext<'_>) -> Result<(), EvalError> {
  for (index, row) in group.iter().enumerate() {
    for cell in &row.cells {
      if index + cell.row_span as usize > group.len() {
        return Err(EvalError::TableRowSpanOverflow {
          span: ctx.span_of(cell.id).into(),
          row_span: cell.row_span,
          remaining: group.len() - index,
        });
      }
    }
  }
  for pair in group.windows(2) {
    if let [above, below] = pair
      && below.separator.is_some()
      && let Some(cell) = first_rule_below(above)
    {
      return Err(EvalError::TableRuleBelowWithSeparator {
        span: ctx.span_of(cell.id).into(),
        separator: ctx.span_of(below.id).into(),
      });
    }
  }
  if let Some(cell) = group.last().and_then(first_rule_below) {
    return Err(EvalError::TableRuleBelowOnFrameBoundary {
      span: ctx.span_of(cell.id).into(),
    });
  }
  return Ok(());
}

/// 行の最初の `rule_below` セルを返す
fn first_rule_below(row: &HirTableRow) -> Option<&HirTableCell> {
  return row.cells.iter().find(|cell| return cell.rule_below);
}
