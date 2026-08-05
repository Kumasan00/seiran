//! 数式環境 — `matrix`
//!
//! 行と列に分割する非採番の数式環境。

use super::math_grid::{GridSpec, evaluate_grid, into_unnumbered_rows};
use crate::{
  frontend::{
    evaluator::{
      EvalError,
      opt_args::{OptType, collect_environment_opt_args, find_string},
    },
    span_ext::ToSourceSpan,
    syntax::ast::EnvironmentView,
  },
  model::{HirBuilder, HirNode, HirNodeKind, MathDelimiter, MathEnvKind},
};

/// `matrix` 環境を評価する
///
/// # Errors
///
/// 未知の任意引数キー・`delimiter` の不正値・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(crate) fn matrix(view: &EnvironmentView, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("delimiter", OptType::String)])?;
  let delimiter = match find_string(&opt_args, "delimiter") {
    Some(value) => MathDelimiter::from_opt_str(&value).ok_or_else(|| {
      return EvalError::InvalidOptArgValue {
        name: "matrix".to_string(),
        key: "delimiter".to_string(),
        expected: "none / paren / bracket / brace / bar / dbar".to_string(),
        span: view.span().to_source_span(),
      };
    })?,
    None => MathDelimiter::None,
  };
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "matrix".to_string(),
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

  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::MathBlock {
      kind: MathEnvKind::Matrix { delimiter },
      rows,
      numbered: false,
      label: None,
    },
  )]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    frontend::evaluator::lookup_env_parse_mode,
    model::{DocNode, MathDelimiter, MathEnvKind},
  };

  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// 結果の最初の `MathBlock` の `(delimiter, rows)` を取り出すヘルパ（kind が Matrix であることも検証）
  fn matrix_of(result: &[DocNode]) -> (MathDelimiter, &[crate::model::MathRow]) {
    let DocNode::MathBlock {
      kind,
      rows,
      numbered,
      ..
    } = &result[0]
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert!(!numbered, "matrix は非採番（環境番号なし）");
    let MathEnvKind::Matrix { delimiter } = kind else {
      panic!("matrix は MathEnvKind::Matrix: {kind:?}");
    };
    return (*delimiter, rows);
  }

  #[test]
  fn matrix_splits_grid_default_delimiter_none_unnumbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{matrix}a & b \\ c & d\end{matrix}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children_to_doc_nodes(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let (delimiter, rows) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::None, "既定は区切りなし");
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 2), "各行 2 列: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "非採番のはず: {rows:?}");
  }

  #[test]
  fn matrix_parses_delimiter_option() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter=bracket]a & b \\ c & d\end{matrix}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children_to_doc_nodes(source, cst).unwrap();

    // Assert
    let (delimiter, _) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::Bracket);
  }

  #[test]
  fn matrix_rejects_unknown_delimiter_value() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter=angle]a & b\end{matrix}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children_to_doc_nodes(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "delimiter"));
  }

  #[test]
  fn matrix_rejects_unknown_opt_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{matrix}[numbered=true]a & b\end{matrix}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children_to_doc_nodes(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "numbered"));
  }
}
