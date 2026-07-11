//! 数式環境 — `split`
//!
//! `\begin{split}...\end{split}` を [`DocNode::MathBlock`]（`kind = Split`）に変換します。本体は
//! `\\` で行・`&` で列に分割し、`align` と同じ列整列（奇数列＝右・偶数列＝左）で揃えますが、採番は
//! **環境全体に 1 つ**だけ（縦中央配置）です。実体は共通ハンドラ
//! [`super::math_grid::evaluate_math_env`]（`NumberingMode::SingleEnv`）に委譲します。
//!
//! ## 任意引数
//!
//! - `[numbered=false]` — 環境全体を無採番にする

use document::{DocNode, MathEnvKind};
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, NumberingMode, evaluate_math_env};
use crate::evaluator::{EvalError, Evaluator};

/// `split` 環境を評価する
///
/// 本体を `\\` で行・`&` で列に分割し、環境全体に 1 つだけ通し番号を発番した `MathBlock`
/// （`kind = Split`）を返す。番号は `layout` 段がブロック縦中央へ配置する。`[numbered=false]` 指定時は
/// 採番しない。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(crate) fn split(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  return evaluate_math_env(
    view,
    evaluator,
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
  use document::MathEnvKind;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// 最初の `DocNode::MathBlock`（`Split`）を分解して (`rows`, `numbered`) を返す
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
    assert_eq!(*kind, MathEnvKind::Split, "split は MathEnvKind::Split");
    return (rows, *numbered);
  }

  #[test]
  fn split_aligns_columns_and_numbers_whole_env_once() {
    // Arrange — 2 行 × 2 列の split。採番は環境全体に 1 つ
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 2 行・各行は無採番・環境全体が採番対象
    let (rows, numbered) = block_of(&result);
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| !r.numbered), "行は無採番: {rows:?}");
    assert!(numbered, "環境全体は採番対象");
  }

  #[test]
  fn split_numbered_false_suppresses_numbering() {
    // Arrange — `[numbered=false]` で環境全体が無採番になる
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false]a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let (rows, numbered) = block_of(&result);
    assert!(rows.iter().all(|r| !r.numbered));
    assert!(!numbered, "無採番のはず");
  }

  #[test]
  fn split_with_label_captures_block_label() {
    // Arrange — `[label=...]` で環境単位ラベルが付き、環境全体が採番対象になる
    let arena = Bump::new();
    let source = r"\begin{split}[label=eq:s]a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — MathBlock.label と numbered が両方付く（行は無採番）
    let DocNode::MathBlock {
      rows,
      numbered,
      label,
      ..
    } = &result[0]
    else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(label.as_deref(), Some("eq:s"), "環境単位ラベルが付く");
    assert!(*numbered, "環境全体は採番対象");
    assert!(rows.iter().all(|r| !r.numbered), "行は無採番: {rows:?}");
  }

  #[test]
  fn split_numbered_false_with_label_errors() {
    // Arrange — 無採番の環境にラベルを付けるとエラー（参照番号が存在しないため）
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false][label=eq:s]a &= b\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "split"));
  }

  #[test]
  fn split_rejects_row_label_marker() {
    // Arrange — split は環境単位採番。行末マーカー `\label{...}` は使えない（`[label=...]` を使う）
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \label{eq:s}\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator;

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
