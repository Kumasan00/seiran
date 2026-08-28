//! 数式環境 — `align`
//!
//! 行と列に分割し、行単位で採番する。

use crate::{
  document::{HirBuilder, HirNode, MathEnvKind},
  frontend::{
    evaluator::{
      EvalError,
      environment::math::math_grid::{GridSpec, NumberingMode, evaluate_math_env},
    },
    syntax::view::EnvironmentView,
  },
};

/// `align` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(crate) fn align(view: &EnvironmentView<'_>, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  return evaluate_math_env(
    view,
    builder,
    MathEnvKind::Align,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: true,
    },
    &NumberingMode::PerRow,
  );
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirMathKind, HirMathRow, HirNodeKind, MathEnvKind},
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  /// 結果の最初の `HirNodeKind::MathBlock`（`Align`）の行スライスを取り出すヘルパ
  fn rows_of(result: &[HirNode]) -> &[HirMathRow] {
    let HirNodeKind::MathBlock { kind, rows, .. } = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Align, "align は MathEnvKind::Align");
    return rows;
  }

  #[test]
  fn align_splits_rows_and_columns_and_numbers_each_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].cells.len(), 2, "行 0 は 2 列");
    assert_eq!(rows[1].cells.len(), 2, "行 1 は 2 列");
    assert!(rows[0].numbered);
    assert!(rows[1].numbered);
  }

  #[test]
  fn align_single_row_is_numbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}x &= y\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    assert!(rows[0].numbered);
  }

  #[test]
  fn align_drops_trailing_blank_row_from_trailing_break() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{align}a &= b \\\\\n\\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 1, "末尾の空行が除去される: {rows:?}");
    assert!(rows[0].numbered);
  }

  #[test]
  fn align_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| return !r.numbered), "無採番のはず: {rows:?}");
  }

  #[test]
  fn align_cell_content_is_evaluated() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}x^{2} &= y\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert!(
      rows[0].cells[0].iter().any(|n| matches!(n.kind, HirMathKind::Superscript(_))),
      "左セルに Superscript ノードが含まれるべき: {:?}",
      rows[0].cells[0]
    );
  }

  #[test]
  fn align_rejects_env_level_label_opt_arg() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[label=eq:foo]a &= b\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn align_row_label_captures_label_and_keeps_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \label{eq:foo} \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].label.as_deref(), Some("eq:foo"));
    assert!(rows[0].numbered);
    assert!(rows[1].label.is_none(), "2 行目はラベルなし: {:?}", rows[1].label);
    assert!(rows[1].numbered);
  }

  #[test]
  fn align_row_label_on_notag_row_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \notag \label{eq:x}\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "align"));
  }

  #[test]
  fn align_row_label_with_numbered_false_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \label{eq:x}\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "align"));
  }

  #[test]
  fn align_row_label_not_at_row_end_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a \label{eq:x} &= b\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotAtRowEnd { .. })));
  }

  #[test]
  fn align_duplicate_row_label_is_structured_without_error() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \label{eq:x} \\ c &= d \label{eq:x}\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows[0].label.as_deref(), Some("eq:x"));
    assert_eq!(rows[1].label.as_deref(), Some("eq:x"));
  }

  #[test]
  fn align_notag_suppresses_single_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d \notag \\ e &= f\end{align}";
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
  fn align_notag_not_at_row_end_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a \notag &= b\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotAtRowEnd { .. })));
  }

  #[test]
  fn align_notag_with_numbered_false_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \notag \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagWithUnnumberedEnv { .. })));
  }
}
