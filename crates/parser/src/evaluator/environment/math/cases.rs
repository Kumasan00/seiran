//! 数式環境 — `cases`
//!
//! `\begin{cases}...\end{cases}` を [`DocNode::MathBlock`]（`kind = Cases`）に変換します。本体は
//! [`syntax::ParseMode::Math`] で構造化された CST を行区切り `\\` × 列区切り `&` のグリッドに分割し、
//! 各行を「式 & 条件」の 2 列固定として扱います（3 列以上は [`EvalError::CasesColumnOverflow`]）。
//! 左の大波括弧と 2 列の左揃えは `layout` 段が [`MathEnvKind::Cases`] に応じて確定します。
//!
//! `cases` は**非採番**であり、`read_style::CounterName::Equation` を一切消費しません（採番ありの
//! 数式環境と通し番号を共有しない）。任意引数も位置引数も受け付けません。

use document::{DocNode, MathEnvKind};
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, evaluate_grid, into_unnumbered_rows};
use crate::evaluator::{EvalError, Evaluator, opt_args::collect_environment_opt_args};

/// `cases` 環境を評価する
///
/// 本体を `\\` で行・`&` で列に分割し、各行を非採番の `MathRow` にした `MathBlock`（`kind = Cases`）を
/// 返す。各行は最大 2 列まで（`式 & 条件`）。番号は付かない。
///
/// # Errors
///
/// 任意引数・位置引数の指定、本体のセル評価失敗、3 列以上の行が現れた場合にエラーを返します
pub(crate) fn cases(view: &EnvironmentView, _evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  // cases は任意引数を受け付けない（未知キーはエラー）
  collect_environment_opt_args(view, &[])?;
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "cases".to_string(),
      span: view.span().into(),
    });
  }

  let source = view.source();
  let grid = match view.body() {
    Some(body_node) => evaluate_grid(
      source,
      body_node,
      &GridSpec {
        allow_row_breaks: true,
        allow_column_breaks: true,
      },
      // cases は非採番のため行末マーカー `\notag` / `\label` は不可
      false,
    )?,
    None => Vec::new(),
  };
  let rows = into_unnumbered_rows(grid);
  // cases は「式 & 条件」の 2 列固定。3 列以上はエラーにする
  if let Some(row) = rows.iter().find(|row| row.cells.len() > 2) {
    return Err(EvalError::CasesColumnOverflow {
      found: row.cells.len(),
      span: view.span().into(),
    });
  }

  return Ok(vec![DocNode::MathBlock {
    kind: MathEnvKind::Cases,
    rows,
    numbered: false,
    // cases は非採番のため参照対象外
    label: None,
    span: view.span().into(),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::MathEnvKind;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn rows_of(result: &[DocNode]) -> &[document::MathRow] {
    let DocNode::MathBlock {
      kind,
      rows,
      numbered,
      ..
    } = &result[0]
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Cases, "cases は MathEnvKind::Cases");
    assert!(!numbered, "cases は非採番（環境番号なし）");
    return rows;
  }

  #[test]
  fn cases_splits_rows_and_two_columns_unnumbered() {
    // Arrange — 2 行・各 2 列（式 & 条件）の cases
    let arena = Bump::new();
    let source = r"\begin{cases}a & x > 0 \\ b & x < 0\end{cases}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 2 行・各 2 セル・全行 numbered は false
    assert_eq!(result.len(), 1);
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert!(rows.iter().all(|r| r.cells.len() == 2), "各行 2 列: {rows:?}");
    assert!(rows.iter().all(|r| !r.numbered), "非採番のはず: {rows:?}");
  }

  #[test]
  fn cases_rejects_three_columns() {
    // Arrange — 1 行 3 列はエラー（cases は 2 列固定）
    let arena = Bump::new();
    let source = r"\begin{cases}a & b & c\end{cases}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::CasesColumnOverflow { found: 3, .. })));
  }

  #[test]
  fn cases_rejects_unknown_opt_key() {
    // Arrange — cases は任意引数を受け付けない
    let arena = Bump::new();
    let source = r"\begin{cases}[foo=1]a & b\end{cases}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn cases_rejects_notag() {
    // Arrange — cases は非採番のため \notag は使えない
    let arena = Bump::new();
    let source = r"\begin{cases}a & b \notag\end{cases}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotSupported { .. })));
  }
}
