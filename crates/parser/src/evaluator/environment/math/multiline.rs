//! 数式環境 — `multiline`
//!
//! `\begin{multiline}...\end{multiline}` を [`DocNode::MathBlock`]（`kind = Multiline`）に変換します。
//! 長い 1 つの式を `\\` で複数行に折り、先頭行を左・末尾行を右・中間行を中央へ配置する「階段」レイアウト
//! を取ります（列区切り `&` は不可）。採番は **環境全体に 1 つ**だけ（縦中央配置）。実体は共通ハンドラ
//! [`super::math_grid::evaluate_math_env`]（`NumberingMode::SingleEnv`）に委譲します。階段配置は
//! `layout` 段が [`MathEnvKind::Multiline`] に応じて確定します。
//!
//! 綴りは amsmath の `multline`（`i` 抜け）ではなく正しい `multiline` を採用します（親 issue #25）。
//!
//! ## 任意引数
//!
//! - `[numbered=false]` — 環境全体を無採番にする

use document::{DocNode, MathEnvKind};
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, NumberingMode, evaluate_math_env};
use crate::evaluator::{EvalError, Evaluator};

/// `multiline` 環境を評価する
///
/// 本体を `\\` で行に分割（`&` は不可）し、環境全体に 1 つだけ通し番号を発番した `MathBlock`
/// （`kind = Multiline`）を返す。階段配置と番号の縦中央配置は `layout` 段が決める。`[numbered=false]`
/// 指定時は採番しない。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体への `&`（列区切り）混入、セル評価失敗時にエラーを返します
pub(crate) fn multiline(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  return evaluate_math_env(
    view,
    evaluator,
    MathEnvKind::Multiline,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: false,
    },
    &NumberingMode::SingleEnv,
  );
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

  fn block_of(result: &[DocNode]) -> (&[document::MathRow], bool) {
    let DocNode::MathBlock {
      kind,
      rows,
      numbered,
      ..
    } = &result[0]
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Multiline, "multiline は MathEnvKind::Multiline");
    return (rows, *numbered);
  }

  #[test]
  fn multiline_splits_rows_single_cell_and_numbers_whole_env_once() {
    // Arrange — 3 行・各 1 セルの multiline。採番は環境全体に 1 つ
    let arena = Bump::new();
    let source = r"\begin{multiline}a + b \\ + c + d \\ + e\end{multiline}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 3 行・各 1 セル・行は無採番・環境全体が採番対象
    let (rows, numbered) = block_of(&result);
    assert_eq!(rows.len(), 3, "3 行: {rows:?}");
    assert!(rows.iter().all(|r| r.cells.len() == 1), "各行 1 セル: {rows:?}");
    assert!(rows.iter().all(|r| !r.numbered), "行は無採番: {rows:?}");
    assert!(numbered);
  }

  #[test]
  fn multiline_rejects_column_break() {
    // Arrange — multiline は `&`（列区切り）を許さない
    let arena = Bump::new();
    let source = r"\begin{multiline}a & b\end{multiline}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn multiline_numbered_false_suppresses_numbering() {
    // Arrange — `[numbered=false]` で環境全体が無採番になる
    let arena = Bump::new();
    let source = r"\begin{multiline}[numbered=false]a + b \\ + c\end{multiline}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let (_, numbered) = block_of(&result);
    assert!(!numbered, "無採番のはず");
  }

  #[test]
  fn multiline_with_label_captures_block_label() {
    // Arrange — `[label=...]` で環境単位ラベルが付き、環境全体が採番対象になる
    let arena = Bump::new();
    let source = r"\begin{multiline}[label=eq:m]a + b \\ + c\end{multiline}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — MathBlock.label と numbered が両方付く
    let DocNode::MathBlock {
      numbered, label, ..
    } = &result[0]
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(label.as_deref(), Some("eq:m"), "環境単位ラベルが付く");
    assert!(*numbered, "環境全体は採番対象");
  }

  #[test]
  fn multiline_rejects_row_label_marker() {
    // Arrange — multiline は環境単位採番。行末マーカー `\label{...}` は使えない（`[label=...]` を使う）
    let arena = Bump::new();
    let source = r"\begin{multiline}a + b \label{eq:m} \\ + c\end{multiline}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
