//! 数式環境 — `split`
//!
//! 行と列を分割し、環境全体を 1 単位として採番する。

use super::math_grid::{GridSpec, NumberingMode, evaluate_math_env};
use crate::{
  frontend::{evaluator::EvalError, syntax::ast::EnvironmentView},
  model::{HirBuilder, HirNode, MathEnvKind},
};

/// `split` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(crate) fn split(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  return evaluate_math_env(
    view,
    builder,
    MathEnvKind::Split,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: true,
    },
    &NumberingMode::SingleEnv,
  );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::evaluator::{evaluate_children_to_hir, lookup_env_parse_mode},
    model::{HirMathRow, HirNodeKind, MathEnvKind},
  };

  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// 最初の `HirNodeKind::MathBlock`（`Split`）を分解して (`rows`, `numbered`) を返す
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
    assert_eq!(*kind, MathEnvKind::Split, "split は MathEnvKind::Split");
    return (rows, *numbered);
  }

  #[test]
  fn split_aligns_columns_and_numbers_whole_env_once() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (rows, numbered) = block_of(&result);
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "行は無採番: {rows:?}");
    assert!(numbered, "環境全体は採番対象");
  }

  #[test]
  fn split_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false]a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (rows, numbered) = block_of(&result);
    assert!(rows.iter().all(|r| return !r.numbered));
    assert!(!numbered, "無採番のはず");
  }

  #[test]
  fn split_with_label_captures_block_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}[label=eq:s]a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::MathBlock {
      rows,
      numbered,
      label,
      ..
    } = &result[0].kind
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(label.as_deref(), Some("eq:s"), "環境単位ラベルが付く");
    assert!(*numbered, "環境全体は採番対象");
    assert!(rows.iter().all(|r| return !r.numbered), "行は無採番: {rows:?}");
  }

  #[test]
  fn split_numbered_false_with_label_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false][label=eq:s]a &= b\end{split}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "split"));
  }

  #[test]
  fn split_rejects_row_label_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \label{eq:s}\end{split}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
