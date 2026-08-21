//! 数式環境 — `multiline`
//!
//! 複数行に分割し、環境全体を 1 単位として採番する。

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

/// `multiline` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体への `&`（列区切り）混入、セル評価失敗時にエラーを返します
pub(crate) fn multiline(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  return evaluate_math_env(
    view,
    builder,
    MathEnvKind::Multiline,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: false,
    },
    &NumberingMode::SingleEnv,
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

  fn block_of(result: &[HirNode]) -> (&[HirMathRow], bool) {
    let HirNodeKind::MathBlock {
      kind,
      rows,
      numbered,
      ..
    } = &result[0].kind
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Multiline, "multiline は MathEnvKind::Multiline");
    return (rows, *numbered);
  }

  #[test]
  fn multiline_splits_rows_single_cell_and_numbers_whole_env_once() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}a + b \\ + c + d \\ + e\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (rows, numbered) = block_of(&result);
    assert_eq!(rows.len(), 3, "3 行: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 1), "各行 1 セル: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "行は無採番: {rows:?}");
    assert!(numbered);
  }

  #[test]
  fn multiline_rejects_column_break() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}a & b\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn multiline_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}[numbered=false]a + b \\ + c\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (_, numbered) = block_of(&result);
    assert!(!numbered, "無採番のはず");
  }

  #[test]
  fn multiline_with_label_captures_block_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}[label=eq:m]a + b \\ + c\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::MathBlock {
      numbered, label, ..
    } = &result[0].kind
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(label.as_deref(), Some("eq:m"), "環境単位ラベルが付く");
    assert!(*numbered, "環境全体は採番対象");
  }

  #[test]
  fn multiline_rejects_row_label_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}a + b \label{eq:m} \\ + c\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
