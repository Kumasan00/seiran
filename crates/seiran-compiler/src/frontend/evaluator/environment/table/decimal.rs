//! `decimal` 列の値のセル（本体行の `column_span == 1` のセル）の中身の検査

use crate::{
  document::{ColumnAlign, HirInline, HirInlineKind, HirMathKind, HirTableCell, HirTableRow},
  frontend::evaluator::{EvalContext, EvalError},
};

/// 裸の数字列とみなす字（`0-9 + - . e E ,`）
const NUMERIC_CHARS: &str = "0123456789+-.eE,";

/// 本体行のうち、`decimal` 列で始まり `column_span == 1` のセル（列の値のセル）を行順・セル順に返す
///
/// 列位置は各行のセル（行結合に覆われる位置の空のセルを含む）の `column_span` の累積で数える。
pub(in crate::frontend) fn value_cells<'a>(
  columns: &'a [ColumnAlign],
  rows: &'a [HirTableRow],
) -> impl Iterator<Item = &'a HirTableCell> {
  return rows.iter().flat_map(move |row| {
    let mut column = 0usize;
    return row.cells.iter().filter(move |cell| {
      let start = column;
      column += cell.column_span as usize;
      return cell.column_span == 1 && columns.get(start) == Some(&ColumnAlign::Decimal);
    });
  });
}

/// 数値要素（`\num` / `\qty`）か
fn is_numeric(inline: &HirInline) -> bool {
  return matches!(inline.kind, HirInlineKind::Number(_) | HirInlineKind::Quantity { .. });
}

/// 数値要素の後ろに続けてよい要素（脚注・索引）か
fn is_trailing_mark(inline: &HirInline) -> bool {
  return matches!(inline.kind, HirInlineKind::Footnote { .. } | HirInlineKind::Index { .. });
}

/// 列の値の中身が許可リストの形か
fn is_value_shape(content: &[HirInline]) -> bool {
  if content.is_empty() {
    return true;
  }
  let marks = content.iter().rev().take_while(|inline| return is_trailing_mark(inline)).count();
  let head = &content[..content.len() - marks];
  return match head {
    [only] if is_numeric(only) => true,
    [only] => match &only.kind {
      HirInlineKind::Colored { children, .. } => matches!(children.as_slice(), [inner] if is_numeric(inner)),
      _ => false,
    },
    _ => false,
  };
}

/// 列の値の中身が許可リストの形でなければ、違反のラベルの文言を返す（`None` は許可）
pub(in crate::frontend) fn column_value_violation(content: &[HirInline]) -> Option<&'static str> {
  if is_value_shape(content) {
    return None;
  }
  let marks = content.iter().rev().take_while(|inline| return is_trailing_mark(inline)).count();
  if let [only] = &content[..content.len() - marks]
    && let HirInlineKind::Text(text) = &only.kind
    && text.chars().all(|c| return NUMERIC_CHARS.contains(c))
    && text.chars().any(|c| return c.is_ascii_digit())
  {
    return Some("裸の数字は数値として読みません。\\num{…} で書いてください");
  }
  let math_with_numeric = content.iter().any(|inline| {
    let HirInlineKind::Math(math) = &inline.kind else {
      return false;
    };
    return math.iter().any(|m| return matches!(m.kind, HirMathKind::Number(_) | HirMathKind::Quantity { .. }));
  });
  if math_with_numeric {
    return Some("数式の中の数値は小数点で揃えません。$ を外して \\num / \\qty をそのまま書いてください");
  }
  return Some("このセルは decimal 列の値です");
}

/// 表全体の列の値のセルを検査し、最初の違反 1 件で止める
///
/// # Errors
///
/// 列の値のセルの中身が許可リストの形でないとき [`EvalError::TableDecimalCell`] を返す。
pub(super) fn check_column_values(
  columns: &[ColumnAlign],
  rows: &[HirTableRow],
  ctx: &EvalContext<'_>,
) -> Result<(), EvalError> {
  for cell in value_cells(columns, rows) {
    if let Some(label) = column_value_violation(&cell.content) {
      return Err(EvalError::TableDecimalCell {
        span: ctx.span_of(cell.id).into(),
        label: label.to_string(),
      });
    }
  }
  return Ok(());
}
