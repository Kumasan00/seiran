//! 数式環境 — `equation`
//!
//! `\begin{equation}...\end{equation}` を [`DocNode::MathBlock`]（`kind = Equation`、1 行 1 セル）に
//! 変換します。本体は [`syntax::ParseMode::Math`] で構造化された CST を
//! [`super::math_grid::evaluate_grid`] で評価します。`equation` は行区切り `\\`・列区切り `&` を
//! 許さないため、本体にそれらが現れるとエラーになります（複数行は `align` 等を使う）。
//!
//! ## 任意引数
//!
//! - `[label=eq:foo]` — `\ref` 解決用ラベル（任意）

use document::{DocNode, MathEnvKind, MathRow};
use read_style::CounterName;
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, evaluate_grid};
use crate::evaluator::{
  EvalError, Evaluator,
  opt_args::{OptType, collect_environment_opt_args, find_string},
};

/// `equation` 環境を評価する
///
/// [`CounterRegistry::increment`] で `CounterName::Equation` の通し番号を発番し、
/// `[label=...]` 指定時はそのレジストリにラベルを登録する。番号書式は
/// `read_style::CounterStyle.format` テンプレ（既定 `"{chapter}.{n}"`）に従う。本体は
/// 単一行・単一セルとして評価し、`MathRow` 1 件の `MathBlock` を返す。
///
/// # Errors
///
/// 不明な任意引数キーや値の型不一致、本体への `&` / `\\` の混入時にエラーを返します
pub(super) fn equation(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("label", OptType::String)])?;
  let label = find_string(opt_args, "label");
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "equation".to_string(),
      span: view.span().into(),
    });
  }

  let number = evaluator
    .registry
    .increment_with_label(CounterName::Equation, label.as_deref(), view.span().into())?;

  let source = view.source();
  // equation は単一行・単一セル。行区切り `\\`・列区切り `&` はエラーにする
  let cells = match view.body() {
    Some(body_node) => {
      let spec = GridSpec {
        allow_row_breaks: false,
        allow_column_breaks: false,
      };
      let grid = evaluate_grid(source, body_node, &spec)?;
      // 分割を許していないので必ず 1 行。その行のセル列（1 セル）を取り出す
      grid.into_iter().next().unwrap_or_else(|| vec![Vec::new()])
    },
    None => vec![Vec::new()],
  };

  let row = MathRow {
    cells,
    number: Some(number),
    label,
  };
  return Ok(vec![DocNode::MathBlock {
    kind: MathEnvKind::Equation,
    rows: vec![row],
  }]);
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
  /// 既定の `"{chapter}.{n}"` はカウンタ経由化の通し番号テスト目的では本質的ではないため、
  /// 番号を素朴な `"1"`, `"2"` 形式に縮約してテストの意図を読みやすくする。
  fn style_with_plain_equation_format() -> Style {
    let mut counters = Counters::default();
    counters.equation.format = "{n}".to_string();
    let style = read_style::Style {
      counters,
      ..Default::default()
    };
    return style;
  }

  /// 結果の最初の `DocNode::MathBlock` から唯一の行を取り出すヘルパ
  fn first_row(result: &[DocNode]) -> &document::MathRow {
    let DocNode::MathBlock { kind, rows } = &result[0] else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(*kind, MathEnvKind::Equation, "equation は MathEnvKind::Equation");
    assert_eq!(rows.len(), 1, "equation は 1 行: {rows:?}");
    return &rows[0];
  }

  #[test]
  fn equation_produces_math_block() {
    // Arrange — 上付き付きの簡単なディスプレイ数式
    let arena = Bump::new();
    let source = r"\begin{equation}x^2 = y\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — MathBlock(Equation) が 1 件、1 行 1 セル、label は None、セルに Superscript
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(row.label.is_none());
    assert_eq!(row.number.as_deref(), Some("1"));
    assert_eq!(row.cells.len(), 1, "equation は 1 セル");
    assert!(
      row.cells[0].iter().any(|n| matches!(n, MathNode::Superscript(_))),
      "Superscript ノードが含まれるべき: {:?}",
      row.cells[0]
    );
  }

  #[test]
  fn equation_with_label_captures_label() {
    // Arrange — label 任意引数を持つ equation
    let arena = Bump::new();
    let source = r"\begin{equation}[label=eq:pythag]a^2+b^2=c^2\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — label と number が両方保持されること
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert_eq!(row.label.as_deref(), Some("eq:pythag"));
    assert_eq!(row.number.as_deref(), Some("1"));
  }

  #[test]
  fn equation_assigns_sequential_numbers() {
    // Arrange — 連続する 2 つの equation は 1, 2 と通し番号が振られる
    let arena = Bump::new();
    let source = r"\begin{equation}a\end{equation}\begin{equation}b\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let numbers: Vec<Option<&str>> = result
      .iter()
      .map(|n| match n {
        DocNode::MathBlock { rows, .. } => rows[0].number.as_deref(),
        _ => panic!("MathBlock が期待されます: {n:?}"),
      })
      .collect();
    assert_eq!(numbers, vec![Some("1"), Some("2")]);
  }

  #[test]
  fn equation_rejects_column_break() {
    // Arrange — equation 本体の `&`（列区切り）はエラー（複数列は align / matrix を使う）
    let arena = Bump::new();
    let source = r"\begin{equation}a & b\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn equation_rejects_row_break() {
    // Arrange — equation 本体の `\\`（行区切り）はエラー（複数行は align / gather を使う）
    let arena = Bump::new();
    let source = r"\begin{equation}a \\ b\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn equation_rejects_unknown_opt_key() {
    // Arrange — equation は label のみ許可、未知キーはエラー
    let arena = Bump::new();
    let source = r"\begin{equation}[foo=1]x\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn equation_number_picks_up_chapter_prefix_via_counter_format() {
    // Arrange — `\chapter` で chapter が 1 に進んだあとの equation は "1.1" になる
    let arena = Bump::new();
    let source = r"\chapter{C}\begin{equation}a\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — Heading 1 件 + MathBlock 1 件、equation の number は既定書式 "{chapter}.{n}" で "1.1"
    assert_eq!(result.len(), 2);
    let DocNode::MathBlock { rows, .. } = &result[1] else {
      panic!("MathBlock が期待されます: {:?}", result[1]);
    };
    assert_eq!(rows[0].number.as_deref(), Some("1.1"));
  }
}
