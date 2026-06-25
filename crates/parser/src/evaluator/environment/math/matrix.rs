//! 数式環境 — `matrix`
//!
//! `\begin{matrix}...\end{matrix}` を [`DocNode::MathBlock`]（`kind = Matrix`）に変換します。本体は
//! [`syntax::ParseMode::Math`] で構造化された CST を行区切り `\\` × 列区切り `&` のグリッドに分割します。
//! 区切り括弧は環境名を分けず（pmatrix / bmatrix … を作らず）、単一 `matrix` 環境の任意引数
//! `[delimiter=none|paren|bracket|brace|bar|dbar]` で選びます（既定 `none`）。中央揃えセルと区切り括弧の
//! 描画は `layout` 段が [`MathEnvKind::Matrix`] に応じて確定します。
//!
//! `matrix` は**非採番**であり、[`read_style::CounterName::Equation`] を一切消費しません。
//!
//! ## 任意引数
//!
//! - `[delimiter=...]` — 区切り括弧の種別（`none` / `paren` / `bracket` / `brace` / `bar` / `dbar`）

use document::{DocNode, MathDelimiter, MathEnvKind};
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, evaluate_grid, into_unnumbered_rows};
use crate::evaluator::{
  EvalError, Evaluator,
  opt_args::{OptType, collect_environment_opt_args, find_string},
};

/// `matrix` 環境を評価する
///
/// 任意引数 `[delimiter=...]` で区切り括弧を選び（既定 `none`）、本体を `\\` で行・`&` で列に分割した
/// 非採番の `MathRow` 列を持つ `MathBlock`（`kind = Matrix { delimiter }`）を返す。
///
/// # Errors
///
/// 未知の任意引数キー・`delimiter` の不正値・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(crate) fn matrix(view: &EnvironmentView, _evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("delimiter", OptType::String)])?;
  let delimiter = match find_string(&opt_args, "delimiter") {
    Some(value) => MathDelimiter::from_opt_str(&value).ok_or_else(|| EvalError::InvalidOptArgValue {
      name: "matrix".to_string(),
      key: "delimiter".to_string(),
      expected: "none / paren / bracket / brace / bar / dbar".to_string(),
      span: view.span().into(),
    })?,
    None => MathDelimiter::None,
  };
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "matrix".to_string(),
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
    )?,
    None => Vec::new(),
  };
  let rows = into_unnumbered_rows(grid);

  return Ok(vec![DocNode::MathBlock {
    kind: MathEnvKind::Matrix { delimiter },
    rows,
    number: None,
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::{MathDelimiter, MathEnvKind};
  use read_style::{Counters, Style};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// equation カウンタの `format` を `"{n}"` に縮約した Style
  fn style_with_plain_equation_format() -> Style {
    let mut counters = Counters::default();
    counters.equation.format = "{n}".to_string();
    return Style {
      counters,
      ..Default::default()
    };
  }

  /// 結果の最初の `MathBlock` の `(delimiter, rows)` を取り出すヘルパ（kind が Matrix であることも検証）
  fn matrix_of(result: &[DocNode]) -> (MathDelimiter, &[document::MathRow]) {
    let DocNode::MathBlock { kind, rows, number } = &result[0] else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert!(number.is_none(), "matrix は非採番（環境番号なし）: {number:?}");
    let MathEnvKind::Matrix { delimiter } = kind else {
      panic!("matrix は MathEnvKind::Matrix: {kind:?}");
    };
    return (*delimiter, rows);
  }

  #[test]
  fn matrix_splits_grid_default_delimiter_none_unnumbered() {
    // Arrange — 2 行 × 2 列のグリッド（区切り指定なし）
    let arena = Bump::new();
    let source = r"\begin{matrix}a & b \\ c & d\end{matrix}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — Matrix { None }、2 行・各 2 セル・非採番
    assert_eq!(result.len(), 1);
    let (delimiter, rows) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::None, "既定は区切りなし");
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| r.cells.len() == 2), "各行 2 列: {rows:?}");
    assert!(rows.iter().all(|r| r.number.is_none()), "非採番のはず: {rows:?}");
  }

  #[test]
  fn matrix_parses_delimiter_option() {
    // Arrange — `[delimiter=bracket]` で角括弧を選ぶ
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter=bracket]a & b \\ c & d\end{matrix}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let (delimiter, _) = matrix_of(&result);
    assert_eq!(delimiter, MathDelimiter::Bracket);
  }

  #[test]
  fn matrix_rejects_unknown_delimiter_value() {
    // Arrange — 未知の区切り値はエラー
    let arena = Bump::new();
    let source = r"\begin{matrix}[delimiter=angle]a & b\end{matrix}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "delimiter"));
  }

  #[test]
  fn matrix_does_not_consume_equation_counter() {
    // Arrange — matrix は非採番。後続 equation は 1 から始まる
    let arena = Bump::new();
    let source = r"\begin{matrix}a & b\end{matrix}\begin{equation}e\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let DocNode::MathBlock { rows: eq_rows, .. } = &result[1] else {
      panic!("equation の MathBlock が期待されます: {:?}", result[1]);
    };
    assert_eq!(eq_rows[0].number.as_deref(), Some("1"));
  }

  #[test]
  fn matrix_rejects_unknown_opt_key() {
    // Arrange — matrix は `[delimiter]` のみ許可。未知キーはエラー
    let arena = Bump::new();
    let source = r"\begin{matrix}[numbered=true]a & b\end{matrix}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "numbered"));
  }
}
