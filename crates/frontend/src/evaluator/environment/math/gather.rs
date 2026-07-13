//! 数式環境 — `gather`
//!
//! `\begin{gather}...\end{gather}` を [`DocNode::MathBlock`]（`kind = Gather`）に変換します。各行は
//! `\\` で分割した単一セル（列区切り `&` は不可）で、各行を `config::read_style::CounterName::Equation` で
//! 採番します。実体は共通ハンドラ [`super::math_grid::evaluate_math_env`]（`NumberingMode::PerRow`）に
//! 委譲します。各行の中央寄せは `layout` 段が [`MathEnvKind::Gather`] に応じて確定します。
//!
//! ## 任意引数・行マーカー
//!
//! - `[numbered=false]` — 環境**全体**を無採番にする
//! - `\notag` — 行の**末尾**に置くと、その**行だけ**を無採番にする（採番カウンタも消費しないため、他行の
//!   通し番号は連続する）。`[numbered=false]` との併用は冗長なためエラー

use document::{DocNode, MathEnvKind};

use super::math_grid::{GridSpec, NumberingMode, evaluate_math_env};
use crate::{evaluator::EvalError, syntax::ast::EnvironmentView};

/// `gather` 環境を評価する
///
/// 本体を `\\` で行に分割（`&` は不可）し、各行に通し番号を発番した `MathBlock`（`kind = Gather`）を
/// 返す。`[numbered=false]` 指定時は採番しない。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体への `&`（列区切り）混入、セル評価失敗時にエラーを返します
pub(crate) fn gather(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
  return evaluate_math_env(
    view,
    MathEnvKind::Gather,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: false,
    },
    &NumberingMode::PerRow,
  );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::MathEnvKind;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn rows_of(result: &[DocNode]) -> &[document::MathRow] {
    let DocNode::MathBlock { kind, rows, .. } = &result[0] else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Gather, "gather は MathEnvKind::Gather");
    return rows;
  }

  #[test]
  fn gather_splits_rows_each_single_cell_and_numbers_each_row() {
    // Arrange — 2 行・各 1 セルの gather
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \\ c = d\end{gather}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — 2 行・各 1 セル・各行採番対象
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert!(rows.iter().all(|r| r.cells.len() == 1), "各行 1 セル: {rows:?}");
    assert!(rows.iter().all(|r| r.numbered));
  }

  #[test]
  fn gather_rejects_column_break() {
    // Arrange — gather は `&`（列区切り）を許さない
    let arena = Bump::new();
    let source = r"\begin{gather}a & b\end{gather}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn gather_numbered_false_suppresses_numbering() {
    // Arrange — `[numbered=false]` で無採番
    let arena = Bump::new();
    let source = r"\begin{gather}[numbered=false]a = b \\ c = d\end{gather}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert!(rows.iter().all(|r| !r.numbered), "無採番のはず: {rows:?}");
  }

  #[test]
  fn gather_notag_suppresses_single_row() {
    // Arrange — gather でも中間行の行末 \notag で 1 行だけ無採番にできる
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \\ c = d \notag \\ e = f\end{gather}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — 採番、無採番、採番
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 3, "3 行に分割される: {rows:?}");
    assert!(rows[0].numbered);
    assert!(!rows[1].numbered, "\\notag 行は無採番のはず");
    assert!(rows[2].numbered);
  }

  #[test]
  fn gather_notag_not_at_row_end_errors() {
    // Arrange — \notag の後ろに内容が続くとエラー
    let arena = Bump::new();
    let source = r"\begin{gather}a \notag = b\end{gather}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotAtRowEnd { .. })));
  }

  #[test]
  fn gather_row_label_captures_label_and_keeps_numbering() {
    // Arrange — gather でも行末マーカー `\label{...}` でその行にラベルを付けられる
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \label{eq:g} \\ c = d\end{gather}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — 1 行目: ラベルあり・採番対象、2 行目: ラベルなし・採番対象
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].label.as_deref(), Some("eq:g"));
    assert!(rows[0].numbered);
    assert!(rows[1].label.is_none(), "2 行目はラベルなし: {:?}", rows[1].label);
    assert!(rows[1].numbered);
  }
}
