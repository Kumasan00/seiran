//! 数式環境 — `matrix`
//!
//! 行と列に分割する非採番の数式環境。

use crate::{
  document::{
    HirMath, HirMathBlock, HirMathKind, HirMathRow, HirNode, HirNodeKind, MathBlockKind, MathDelimiter, NodeId,
  },
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity,
      environment::math::grid::{GridSpec, evaluate_grid, into_unnumbered_rows},
      opt_args::{self, OptKey, collect_environment_opt_args},
    },
    syntax::view::EnvironmentView,
  },
};

/// `matrix[delimiter=...]`（両端の区切り記号）
const DELIMITER: OptKey<String> = opt_args::string("delimiter");

/// `matrix` 環境を表示数式ブロックとして評価する
///
/// # Errors
///
/// 未知の任意引数キー・`delimiter` の不正値・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(in crate::frontend::evaluator::environment) fn matrix(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
) -> Result<HirNode, EvalError> {
  let (id, delimiter, rows) = evaluate_matrix(view, ctx)?;
  return Ok(HirNode::new(
    id,
    HirNodeKind::MathBlock(HirMathBlock {
      kind: MathBlockKind::Matrix { delimiter },
      rows,
      numbered: false,
      label: None,
    }),
  ));
}

/// 数式の中に書いた `matrix` 環境を、数式の 1 要素として評価する
///
/// # Errors
///
/// [`matrix`] と同じ
pub(in crate::frontend::evaluator::environment) fn embedded_matrix(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
) -> Result<HirMath, EvalError> {
  let (id, delimiter, rows) = evaluate_matrix(view, ctx)?;
  return Ok(HirMath::new(
    id,
    HirMathKind::Matrix {
      delimiter,
      rows: rows.into_iter().map(|row| return row.cells).collect(),
    },
  ));
}

/// `matrix` 環境のオプションと本体を評価し、（環境の ID, 区切り括弧, 行）を返す
///
/// 環境の ID はセルより先に確保する。行は非採番で、行末マーカーは受け付けない。
fn evaluate_matrix(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
) -> Result<(NodeId, MathDelimiter, Vec<HirMathRow>), EvalError> {
  let opts = collect_environment_opt_args(view, &[DELIMITER.decl()])?;
  let delimiter = match opts.get(DELIMITER) {
    Some(value) => {
      let Ok(delimiter) = value.parse::<MathDelimiter>() else {
        return Err(EvalError::InvalidOptArgValue {
          name: "matrix".to_string(),
          key: "delimiter".to_string(),
          expected: "none / paren / bracket / brace / bar / dbar".to_string(),
          span: view.span().into(),
        });
      };
      delimiter
    },
    None => MathDelimiter::None,
  };
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
  return Ok((id, delimiter, into_unnumbered_rows(grid)));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirMathRow, MathBlockKind, MathDelimiter},
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  /// 結果の最初の `MathBlock` の `(delimiter, rows)` を取り出すヘルパ
  fn matrix_of(result: &[HirNode]) -> (MathDelimiter, &[HirMathRow]) {
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert!(!math.numbered, "matrix は非採番（環境番号なし）");
    let MathBlockKind::Matrix { delimiter } = math.kind else {
      panic!("matrix は MathBlockKind::Matrix: {:?}", math.kind);
    };
    return (delimiter, &math.rows);
  }

  #[test]
  fn matrix_splits_grid_default_delimiter_none_unnumbered() {
    let arena = Bump::new();
    let source = r"\begin{matrix}a & b \\ c & d\end{matrix}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(result.len(), 1);
    let (delimiter, rows) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::None, "既定は区切りなし");
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 2), "各行 2 列: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "非採番のはず: {rows:?}");
  }

  #[test]
  fn matrix_parses_delimiter_option() {
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter=bracket]a & b \\ c & d\end{matrix}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let (delimiter, _) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::Bracket);
  }

  #[test]
  fn matrix_rejects_unknown_delimiter_value() {
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter=angle]a & b\end{matrix}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "delimiter"));
  }

  #[test]
  fn matrix_rejects_uppercase_delimiter_value() {
    for value in ["PAREN", "Paren"] {
      let arena = Bump::new();
      let source = format!(r"\begin{{matrix}}[delimiter={value}]a & b\end{{matrix}}");
      let cst = test_support::parse_cst(&source, &arena).unwrap();

      let result = evaluate_children_to_hir(&source, cst);

      assert!(
        matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "delimiter"),
        "{value} は delimiter の値不正になるはず: {result:?}"
      );
    }
  }

  #[test]
  fn matrix_accepts_delimiter_value_with_surrounding_spaces() {
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter= paren ]a & b\end{matrix}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst).unwrap();

    let (delimiter, _) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::Paren);
  }

  #[test]
  fn matrix_rejects_unknown_opt_key() {
    let arena = Bump::new();
    let source = r"\begin{matrix}[numbered=true]a & b\end{matrix}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "numbered"));
  }
}
