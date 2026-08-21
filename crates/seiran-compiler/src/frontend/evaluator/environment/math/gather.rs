//! 数式環境 — `gather`
//!
//! 単一セルの行に分割し、行単位で採番する。

use crate::{
  document::{HirBuilder, HirNode, MathEnvKind},
  frontend::{
    evaluator::{
      EvalError,
      environment::math::math_grid::{GridSpec, NumberingMode, evaluate_math_env},
    },
    syntax::ast::EnvironmentView,
  },
};

/// `gather` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体への `&`（列区切り）混入、セル評価失敗時にエラーを返します
pub(crate) fn gather(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  return evaluate_math_env(
    view,
    builder,
    MathEnvKind::Gather,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: false,
    },
    &NumberingMode::PerRow,
  );
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirMathRow, HirNodeKind, MathEnvKind},
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  fn rows_of(result: &[HirNode]) -> &[HirMathRow] {
    let HirNodeKind::MathBlock { kind, rows, .. } = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Gather, "gather は MathEnvKind::Gather");
    return rows;
  }

  #[test]
  fn gather_splits_rows_each_single_cell_and_numbers_each_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \\ c = d\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 1), "各行 1 セル: {rows:?}");
    assert!(rows.iter().all(|r| return r.numbered));
  }

  #[test]
  fn gather_rejects_column_break() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a & b\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn gather_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}[numbered=false]a = b \\ c = d\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert!(rows.iter().all(|r| return !r.numbered), "無採番のはず: {rows:?}");
  }

  #[test]
  fn gather_notag_suppresses_single_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \\ c = d \notag \\ e = f\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 3, "3 行に分割される: {rows:?}");
    assert!(rows[0].numbered);
    assert!(!rows[1].numbered, "\\notag 行は無採番のはず");
    assert!(rows[2].numbered);
  }

  #[test]
  fn gather_notag_not_at_row_end_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a \notag = b\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotAtRowEnd { .. })));
  }

  #[test]
  fn gather_row_label_captures_label_and_keeps_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \label{eq:g} \\ c = d\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].label.as_deref(), Some("eq:g"));
    assert!(rows[0].numbered);
    assert!(rows[1].label.is_none(), "2 行目はラベルなし: {:?}", rows[1].label);
    assert!(rows[1].numbered);
  }
}
