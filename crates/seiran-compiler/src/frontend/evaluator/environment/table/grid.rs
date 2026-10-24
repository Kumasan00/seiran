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

/// 行群（`\head` か本体）の行結合・区切り・部分罫の境界を検査する
///
/// 結合は行群の最後の行を超えられない。`\head` と本体は別の行群なので、またぐ結合もここで拒否される。
/// 境界 k（行 k と k + 1 の間）を結合セルがまたぐときは、行 k + 1 に `separator` を置けない。
/// `rule_below` は結合の最後の行の下で判定し、それが行群の最後（枠罫の持ち場）か、次の行が `separator` を持つ境界には置けない。
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

  // 上の検査で j + row_span <= group.len() なので、k は境界の個数（group.len() - 1）に収まる
  let mut crossing: Vec<Option<NodeId>> = vec![None; group.len().saturating_sub(1)];
  for (index, row) in group.iter().enumerate() {
    for cell in &row.cells {
      for owner in &mut crossing[index..index + cell.row_span as usize - 1] {
        owner.get_or_insert(cell.id);
      }
    }
  }
  for (boundary, owner) in crossing.iter().enumerate() {
    if let Some(owner) = owner
      && let Some(row) = group.get(boundary + 1).filter(|row| return row.separator.is_some())
    {
      return Err(EvalError::TableSeparatorInsideRowSpan {
        span: ctx.span_of(row.id).into(),
        cell: ctx.span_of(*owner).into(),
      });
    }
  }

  for (index, row) in group.iter().enumerate() {
    for cell in row.cells.iter().filter(|cell| return cell.rule_below) {
      let last = index + cell.row_span as usize - 1;
      match group.get(last + 1) {
        None => {
          return Err(EvalError::TableRuleBelowOnFrameBoundary {
            span: ctx.span_of(cell.id).into(),
          });
        },
        Some(next) if next.separator.is_some() => {
          return Err(EvalError::TableRuleBelowWithSeparator {
            span: ctx.span_of(cell.id).into(),
            separator: ctx.span_of(next.id).into(),
          });
        },
        Some(_) => {},
      }
    }
  }
  return Ok(());
}
