//! 数式環境 — `cases`
//!
//! 各行を最大 2 セルに分割する非採番の数式環境。

use crate::{
  document::{HirBuilder, HirNode, HirNodeKind, MathEnvKind},
  frontend::{
    evaluator::{
      EvalError,
      environment::math::math_grid::{GridSpec, evaluate_grid, into_unnumbered_rows},
      opt_args::collect_environment_opt_args,
    },
    span_ext::ToSourceSpan,
    syntax::ast::EnvironmentView,
  },
};

/// `cases` 環境を評価する
///
/// # Errors
///
/// 任意引数・位置引数の指定、本体のセル評価失敗、3 列以上の行が現れた場合にエラーを返します
pub(crate) fn cases(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  collect_environment_opt_args(view, &[])?;
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "cases".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let source = view.source();
  let id = builder.alloc(view.span());
  let grid = match view.body() {
    Some(body_node) => evaluate_grid(
      source,
      builder,
      body_node,
      &GridSpec {
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
      span: view.span().to_source_span(),
    });
  }

  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::MathBlock {
      kind: MathEnvKind::Cases,
      rows,
      numbered: false,
      label: None,
    },
  )]);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirMathRow, MathEnvKind},
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  fn rows_of(result: &[HirNode]) -> &[HirMathRow] {
    let HirNodeKind::MathBlock {
      kind,
      rows,
      numbered,
      ..
    } = &result[0].kind
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Cases, "cases は MathEnvKind::Cases");
    assert!(!numbered, "cases は非採番（環境番号なし）");
    return rows;
  }

  #[test]
  fn cases_splits_rows_and_two_columns_unnumbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{cases}a & x > 0 \\ b & x < 0\end{cases}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 2), "各行 2 列: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "非採番のはず: {rows:?}");
  }

  #[test]
  fn cases_rejects_three_columns() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{cases}a & b & c\end{cases}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::CasesColumnOverflow { found: 3, .. })));
  }

  #[test]
  fn cases_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{cases}[foo=1]a & b\end{cases}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn cases_rejects_notag() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{cases}a & b \notag\end{cases}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotSupported { .. })));
  }
}
