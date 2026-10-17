//! 数式環境 — `cases`
//!
//! 各行を最大 2 セルに分割する非採番の数式環境。

use crate::{
  document::{HirMath, HirMathBlock, HirMathKind, HirMathRow, HirNode, HirNodeKind, MathBlockKind, NodeId},
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity,
      environment::math::grid::{GridSpec, evaluate_grid, into_unnumbered_rows},
      opt_args,
    },
    syntax::view::EnvironmentView,
  },
};

/// `cases` 環境を表示数式ブロックとして評価する
///
/// # Errors
///
/// 任意引数・位置引数の指定、本体のセル評価失敗、3 列以上の行が現れた場合にエラーを返します
pub(in crate::frontend::evaluator::environment) fn cases(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
) -> Result<HirNode, EvalError> {
  let (id, rows) = evaluate_cases(view, ctx)?;
  return Ok(HirNode::new(
    id,
    HirNodeKind::MathBlock(HirMathBlock {
      kind: MathBlockKind::Cases,
      rows,
      numbered: false,
      label: None,
    }),
  ));
}

/// 数式の中に書いた `cases` 環境を、数式の 1 要素として評価する
///
/// # Errors
///
/// [`cases`] と同じ
pub(in crate::frontend::evaluator::environment) fn embedded_cases(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
) -> Result<HirMath, EvalError> {
  let (id, rows) = evaluate_cases(view, ctx)?;
  return Ok(HirMath::new(
    id,
    HirMathKind::Cases {
      rows: rows.into_iter().map(|row| return row.cells).collect(),
    },
  ));
}

/// `cases` 環境のオプションと本体を評価し、（環境の ID, 2 セル以下の行）を返す
///
/// 環境の ID はセルより先に確保する。行は非採番で、行末マーカーは受け付けない。
fn evaluate_cases(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<(NodeId, Vec<HirMathRow>), EvalError> {
  opt_args::no_environment_opt_args(view)?;
  arity::no_environment_args(view)?;

  let source = view.source();
  let id = ctx.alloc(view.span());
  let grid = match view.body() {
    Some(body_node) => evaluate_grid(
      source,
      ctx,
      body_node,
      GridSpec {
        allow_row_breaks: true,
        allow_column_breaks: true,
      },
      false,
    )?,
    None => Vec::new(),
  };
  let rows = into_unnumbered_rows(grid);
  if let Some(row) = rows.iter().find(|row| return row.cells.len() > 2) {
    return Err(EvalError::CasesColumnOverflow {
      found: row.cells.len(),
      span: view.span().into(),
    });
  }
  return Ok((id, rows));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirMathRow, MathBlockKind},
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  fn rows_of(result: &[HirNode]) -> &[HirMathRow] {
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.kind, MathBlockKind::Cases, "cases は MathBlockKind::Cases");
    assert!(!math.numbered, "cases は非採番（環境番号なし）");
    return &math.rows;
  }

  #[test]
  fn cases_splits_rows_and_two_columns_unnumbered() {
    let arena = Bump::new();
    let source = r"\begin{cases}a & x > 0 \\ b & x < 0\end{cases}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 1);
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 2), "各行 2 列: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "非採番のはず: {rows:?}");
  }

  #[test]
  fn cases_rejects_three_columns() {
    let arena = Bump::new();
    let source = r"\begin{cases}a & b & c\end{cases}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::CasesColumnOverflow { found: 3, .. })));
  }

  #[test]
  fn cases_rejects_unknown_opt_key() {
    let arena = Bump::new();
    let source = r"\begin{cases}[foo=1]a & b\end{cases}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn cases_rejects_notag() {
    let arena = Bump::new();
    let source = r"\begin{cases}a & b \notag\end{cases}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::NotagNotSupported { .. })));
  }
}
