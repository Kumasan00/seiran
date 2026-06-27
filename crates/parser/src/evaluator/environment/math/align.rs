//! 数式環境 — `align`
//!
//! `\begin{align}...\end{align}` を [`DocNode::MathBlock`]（`kind = Align`）に変換します。本体は
//! [`syntax::ParseMode::Math`] で構造化された CST を行区切り `\\` × 列区切り `&` のグリッドに分割し、
//! 各行を [`read_style::CounterName::Equation`] で採番します。実体は共通ハンドラ
//! [`super::math_grid::evaluate_math_env`]（`NumberingMode::PerRow`）に委譲します。列整列（奇数列＝右・
//! 偶数列＝左で `&` 位置に接合）は `layout` 段が [`MathEnvKind::Align`] に応じて確定します。
//!
//! ## 任意引数・行マーカー
//!
//! - `[numbered=false]` — 環境**全体**を無採番にする
//! - `\notag` — 行の**末尾**に置くと、その**行だけ**を無採番にする（採番カウンタも消費しないため、他行の
//!   通し番号は連続する）。`[numbered=false]` との併用は冗長なためエラー。行ラベルの行単位指定は将来対応

use document::{DocNode, MathEnvKind};
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, NumberingMode, evaluate_math_env};
use crate::evaluator::{EvalError, Evaluator};

/// `align` 環境を評価する
///
/// 本体を `\\` で行・`&` で列に分割し、各行に [`read_style::CounterName::Equation`] の通し番号を
/// 発番した `MathRow` 列を持つ `MathBlock`（`kind = Align`）を返す。`[numbered=false]` 指定時は採番しない。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価失敗時にエラーを返します
pub(crate) fn align(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  return evaluate_math_env(
    view,
    evaluator,
    MathEnvKind::Align,
    &GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: true,
    },
    &NumberingMode::PerRow,
  );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::{MathEnvKind, MathNode};
  use read_style::{Counters, Style};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// equation カウンタの `format` を `"{n}"` に差し替えた Style を返す
  ///
  /// 既定の `"{chapter}.{n}"` は通し番号の確認には本質的でないため、番号を素朴な `"1"`, `"2"`
  /// 形式に縮約してテストの意図を読みやすくする。
  fn style_with_plain_equation_format() -> Style {
    let mut counters = Counters::default();
    counters.equation.format = "{n}".to_string();
    return Style {
      counters,
      ..Default::default()
    };
  }

  /// 結果の最初の `DocNode::MathBlock`（`Align`）の行スライスを取り出すヘルパ
  fn rows_of(result: &[DocNode]) -> &[document::MathRow] {
    let DocNode::MathBlock { kind, rows, .. } = &result[0] else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Align, "align は MathEnvKind::Align");
    return rows;
  }

  #[test]
  fn align_splits_rows_and_columns_and_numbers_each_row() {
    // Arrange — 2 行 × 2 列の align（`&` で整列、`\\` で行分割）
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — MathBlock(Align) 1 件、2 行・各 2 セル、各行が通し番号を持つ
    assert_eq!(result.len(), 1);
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].cells.len(), 2, "行 0 は 2 列");
    assert_eq!(rows[1].cells.len(), 2, "行 1 は 2 列");
    assert_eq!(rows[0].number.as_deref(), Some("1"));
    assert_eq!(rows[1].number.as_deref(), Some("2"));
  }

  #[test]
  fn align_single_row_is_numbered() {
    // Arrange — 行・列分割のない 1 行の align も採番される
    let arena = Bump::new();
    let source = r"\begin{align}x &= y\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[0].number.as_deref(), Some("1"));
  }

  #[test]
  fn align_drops_trailing_blank_row_from_trailing_break() {
    // Arrange — 行末 `\\`（その後ろは空白のみ）は空行を生むが採番せず捨てる
    let arena = Bump::new();
    let source = "\\begin{align}a &= b \\\\\n\\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 末尾の空行は除去され、採番は 1 行ぶんだけ
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 1, "末尾の空行が除去される: {rows:?}");
    assert_eq!(rows[0].number.as_deref(), Some("1"));
  }

  #[test]
  fn align_numbers_share_equation_counter_with_equation() {
    // Arrange — align の各行と後続 equation が同じ equation カウンタを共有して連番になる
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d\end{align}\begin{equation}e\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — align の 2 行が 1, 2、続く equation が 3
    assert_eq!(result.len(), 2);
    let align_rows = rows_of(&result[..1]);
    assert_eq!(align_rows[0].number.as_deref(), Some("1"));
    assert_eq!(align_rows[1].number.as_deref(), Some("2"));
    let DocNode::MathBlock { rows: eq_rows, .. } = &result[1] else {
      panic!("equation の MathBlock が期待されます: {:?}", result[1]);
    };
    assert_eq!(eq_rows[0].number.as_deref(), Some("3"));
  }

  #[test]
  fn align_numbered_false_suppresses_numbering() {
    // Arrange — `[numbered=false]` で各行が無採番になる
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \\ c &= d\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 2 行とも number は None
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.number.is_none()), "無採番のはず: {rows:?}");
  }

  #[test]
  fn align_cell_content_is_evaluated() {
    // Arrange — セル内の上付きが評価されて MathNode として現れる
    let arena = Bump::new();
    let source = r"\begin{align}x^2 &= y\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 左セルに Superscript が含まれる
    let rows = rows_of(&result);
    assert!(
      rows[0].cells[0].iter().any(|n| matches!(n, MathNode::Superscript(_))),
      "左セルに Superscript ノードが含まれるべき: {:?}",
      rows[0].cells[0]
    );
  }

  #[test]
  fn align_rejects_env_level_label_opt_arg() {
    // Arrange — align は行ごと採番。ラベルは行末マーカー `\label{...}` で付けるため、環境レベルの
    // `[label=...]` は受け付けない（スキーマ外の未知キーエラー）
    let arena = Bump::new();
    let source = r"\begin{align}[label=eq:foo]a &= b\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn align_row_label_captures_label_and_keeps_numbering() {
    // Arrange — 1 行目の行末に `\label{...}` を置くと、その行に番号とラベルが付く
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \label{eq:foo} \\ c &= d\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 1 行目: ラベルあり・番号 1、2 行目: ラベルなし・番号 2（通し番号は連続）
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].label.as_deref(), Some("eq:foo"));
    assert_eq!(rows[0].number.as_deref(), Some("1"));
    assert!(rows[1].label.is_none(), "2 行目はラベルなし: {:?}", rows[1].label);
    assert_eq!(rows[1].number.as_deref(), Some("2"));
  }

  #[test]
  fn align_row_label_on_notag_row_errors() {
    // Arrange — \notag 行（無採番）にラベルは付けられない（参照番号が無いため）
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \notag \label{eq:x}\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "align"));
  }

  #[test]
  fn align_row_label_with_numbered_false_errors() {
    // Arrange — [numbered=false]（全行無採番）の行ラベルも参照番号が無いためエラー
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \label{eq:x}\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "align"));
  }

  #[test]
  fn align_row_label_not_at_row_end_errors() {
    // Arrange — \label の後ろに列・内容が続く（行末でない）とエラー
    let arena = Bump::new();
    let source = r"\begin{align}a \label{eq:x} &= b\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotAtRowEnd { .. })));
  }

  #[test]
  fn align_duplicate_row_label_errors() {
    // Arrange — 同名の行ラベルを 2 行に付けると重複エラー
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \label{eq:x} \\ c &= d \label{eq:x}\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::DuplicateLabel { ref label, .. }) if label == "eq:x"));
  }

  #[test]
  fn align_notag_suppresses_single_row() {
    // Arrange — 中間行の行末に \notag を置くと、その行だけ無採番になり通し番号は連続する
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d \notag \\ e &= f\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — 1 行目 (1)、2 行目 無採番、3 行目 (2)
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 3, "3 行に分割される: {rows:?}");
    assert_eq!(rows[0].number.as_deref(), Some("1"));
    assert!(rows[1].number.is_none(), "\\notag 行は無採番: {:?}", rows[1].number);
    assert_eq!(rows[2].number.as_deref(), Some("2"), "通し番号は連続する");
  }

  #[test]
  fn align_notag_row_does_not_consume_counter() {
    // Arrange — \notag 行はカウンタを消費しないので、後続 equation の採番が飛ばない
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \notag \\ c &= d\end{align}\begin{equation}e\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — align: 1 行目 無採番・2 行目 (1)、続く equation が (2)
    assert_eq!(result.len(), 2);
    let align_rows = rows_of(&result[..1]);
    assert!(align_rows[0].number.is_none(), "\\notag 行は無採番: {:?}", align_rows[0].number);
    assert_eq!(align_rows[1].number.as_deref(), Some("1"));
    let DocNode::MathBlock { rows: eq_rows, .. } = &result[1] else {
      panic!("equation の MathBlock が期待されます: {:?}", result[1]);
    };
    assert_eq!(eq_rows[0].number.as_deref(), Some("2"));
  }

  #[test]
  fn align_notag_not_at_row_end_errors() {
    // Arrange — \notag の後ろに列・内容が続く（行末でない）とエラー
    let arena = Bump::new();
    let source = r"\begin{align}a \notag &= b\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotAtRowEnd { .. })));
  }

  #[test]
  fn align_notag_with_numbered_false_errors() {
    // Arrange — [numbered=false]（全行無採番）と \notag の併用は冗長でエラー
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \notag \\ c &= d\end{align}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagWithUnnumberedEnv { .. })));
  }
}
