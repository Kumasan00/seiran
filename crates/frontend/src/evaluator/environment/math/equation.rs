//! 数式環境 — `equation`
//!
//! 1 行 1 セルとして評価し、行単位で採番する。

use model::{DocNode, MathEnvKind, MathRow};

use super::math_grid::{GridSpec, evaluate_grid};
use crate::{
  evaluator::{
    EvalError,
    opt_args::{OptType, collect_environment_opt_args, find_bool, find_string},
  },
  span_ext::ToSourceSpan,
  syntax::ast::EnvironmentView,
};

/// `equation` 環境を評価する
///
/// # Errors
///
/// 不明な任意引数キーや値の型不一致、本体への `&` / `\\` の混入時にエラーを返します。
/// `[numbered=false]` と `[label=...]` を併用した場合は [`EvalError::LabelRequiresNumbering`] を返します
pub(crate) fn equation(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("label", OptType::String), ("numbered", OptType::Bool)])?;
  let numbered = find_bool(&opt_args, "numbered").unwrap_or(true);
  let label = find_string(&opt_args, "label");
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "equation".to_string(),
      span: view.span().to_source_span(),
    });
  }
  if !numbered && label.is_some() {
    return Err(EvalError::LabelRequiresNumbering {
      name: "equation".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let source = view.source();
  let cells = match view.body() {
    Some(body_node) => {
      let spec = GridSpec {
        allow_row_breaks: false,
        allow_column_breaks: false,
      };
      let grid = evaluate_grid(source, body_node, &spec, false)?;
      grid.into_iter().next().map_or_else(|| vec![Vec::new()], |row| return row.cells)
    },
    None => vec![Vec::new()],
  };

  let row = MathRow {
    cells,
    numbered,
    label,
    label_span: None,
  };
  return Ok(vec![DocNode::MathBlock {
    kind: MathEnvKind::Equation,
    rows: vec![row],
    numbered: false,
    label: None,
    span: view.span(),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use model::{MathEnvKind, MathNode};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// 結果の最初の `DocNode::MathBlock` から唯一の行を取り出すヘルパ
  fn first_row(result: &[DocNode]) -> &model::MathRow {
    let DocNode::MathBlock { kind, rows, .. } = &result[0] else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Equation, "equation は MathEnvKind::Equation");
    assert_eq!(rows.len(), 1, "equation は 1 行: {rows:?}");
    return &rows[0];
  }

  #[test]
  fn equation_produces_math_block() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}x^2 = y\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(row.label.is_none());
    assert!(row.numbered);
    assert_eq!(row.cells.len(), 1, "equation は 1 セル");
    assert!(
      row.cells[0].iter().any(|n| matches!(n, MathNode::Superscript(_))),
      "Superscript ノードが含まれるべき: {:?}",
      row.cells[0]
    );
  }

  #[test]
  fn equation_with_label_captures_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}[label=eq:pythag]a^2+b^2=c^2\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert_eq!(row.label.as_deref(), Some("eq:pythag"));
    assert!(row.numbered);
  }

  #[test]
  fn equation_rejects_column_break() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}a & b\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn equation_rejects_row_break() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}a \\ b\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn equation_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}[foo=1]x\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn equation_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}[numbered=false]x\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(!row.numbered, "無採番なので numbered は false のはず");
  }

  #[test]
  fn equation_numbered_true_is_explicit_default() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}[numbered=true]x\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(row.numbered);
  }

  #[test]
  fn equation_numbered_false_with_label_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}[numbered=false][label=eq:x]a\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "equation"));
  }

  #[test]
  fn equation_rejects_notag() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}a \notag\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotSupported { .. })));
  }

  #[test]
  fn equation_rejects_row_label_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}a \label{eq:x}\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
