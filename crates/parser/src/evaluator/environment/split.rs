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
pub(super) fn split(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
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

  /// 最初の `DocNode::MathBlock`（`Split`）を分解して (`rows`, `env_number`) を返す
  fn block_of(result: &[DocNode]) -> (&[document::MathRow], Option<&str>) {
    let DocNode::MathBlock { kind, rows, number } = &result[0] else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Split, "split は MathEnvKind::Split");
    return (rows, number.as_deref());
  }

  #[test]
  fn split_aligns_columns_and_numbers_whole_env_once() {
    // Arrange — 2 行 × 2 列の split。番号は環境全体に 1 つ
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 2 行・各行は無採番・環境全体の番号が "1"
    let (rows, env_number) = block_of(&result);
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| r.number.is_none()), "行は無採番: {rows:?}");
    assert_eq!(env_number, Some("1"), "環境全体に 1 つの番号");
  }

  #[test]
  fn split_consumes_single_counter_value() {
    // Arrange — split（環境 1 つ分）の後の equation は 2 になる（split は 1 を 1 回だけ消費）
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \\ &= c\end{split}\begin{equation}d\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — split の env number = 1、equation = 2
    let (_, env_number) = block_of(&result);
    assert_eq!(env_number, Some("1"));
    let DocNode::MathBlock { rows: eq_rows, .. } = &result[1] else {
      panic!("equation の MathBlock が期待されます: {:?}", result[1]);
    };
    assert_eq!(eq_rows[0].number.as_deref(), Some("2"));
  }

  #[test]
  fn split_numbered_false_suppresses_numbering() {
    // Arrange — `[numbered=false]` で環境全体の番号も出ない
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false]a &= b \\ &= c\end{split}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let (rows, env_number) = block_of(&result);
    assert!(rows.iter().all(|r| r.number.is_none()));
    assert_eq!(env_number, None, "無採番のはず");
  }
}
