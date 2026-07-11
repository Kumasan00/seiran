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
//! - `[numbered=false]` — 本体を無採番にする（既定は採番あり）。`align` 等 4 環境の `[numbered]` と
//!   同一意味論。無採番のときは equation カウンタを消費しない（後続の採番式の通し番号が連続する）。
//!   無採番の式に `[label=...]` を併用するとエラー（参照番号が存在しないため）

use document::{DocNode, MathEnvKind, MathRow};
use syntax::ast::EnvironmentView;

use super::math_grid::{GridSpec, evaluate_grid};
use crate::evaluator::{
  EvalError,
  opt_args::{OptType, collect_environment_opt_args, find_bool, find_string},
};

/// `equation` 環境を評価する
///
/// `[label=...]` を捕捉し、`[numbered=false]`（既定は採番あり）で採番対象かどうかを決める。
/// 採番の発番・書式化（`CounterName::Equation` の消費、`number_format` テンプレ適用）は行わない
/// （`lowering` 層の責務）。本体は単一行・単一セルとして評価し、`MathRow` 1 件の `MathBlock` を返す。
///
/// # Errors
///
/// 不明な任意引数キーや値の型不一致、本体への `&` / `\\` の混入時にエラーを返します。
/// `[numbered=false]` と `[label=...]` を併用した場合は [`EvalError::LabelRequiresNumbering`] を返します
pub(crate) fn equation(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("label", OptType::String), ("numbered", OptType::Bool)])?;
  let numbered = find_bool(&opt_args, "numbered").unwrap_or(true);
  let label = find_string(&opt_args, "label");
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "equation".to_string(),
      span: view.span().into(),
    });
  }
  // 無採番の式は参照番号を持たないため、ラベルとの併用を禁じる
  if !numbered && label.is_some() {
    return Err(EvalError::LabelRequiresNumbering {
      name: "equation".to_string(),
      span: view.span().into(),
    });
  }

  let source = view.source();
  // equation は単一行・単一セル。行区切り `\\`・列区切り `&` はエラーにする
  let cells = match view.body() {
    Some(body_node) => {
      let spec = GridSpec {
        allow_row_breaks: false,
        allow_column_breaks: false,
      };
      // equation は行ごと採番ではないため行末マーカー `\notag` / `\label` は不可
      // （無採番なら `[numbered=false]`、ラベルは `[label=...]`）
      let grid = evaluate_grid(source, body_node, &spec, false)?;
      // 分割を許していないので必ず 1 行。その行のセル列（1 セル）を取り出す
      grid.into_iter().next().map_or_else(|| vec![Vec::new()], |row| row.cells)
    },
    None => vec![Vec::new()],
  };

  let row = MathRow {
    cells,
    numbered,
    label,
    label_span: None,
  };
  return Ok(vec![DocNode::MathBlock {
    kind: MathEnvKind::Equation,
    rows: vec![row],
    // equation は行ごと採番（`row.numbered`）。環境全体の採番・ラベルは使わない（ラベルは `row.label`）
    numbered: false,
    label: None,
    span: view.span().into(),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use document::{MathEnvKind, MathNode};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// 結果の最初の `DocNode::MathBlock` から唯一の行を取り出すヘルパ
  fn first_row(result: &[DocNode]) -> &document::MathRow {
    let DocNode::MathBlock { kind, rows, .. } = &result[0] else {
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

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — MathBlock(Equation) が 1 件、1 行 1 セル、label は None、既定で採番対象、セルに Superscript
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(row.label.is_none());
    assert!(row.numbered);
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

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — label と numbered が両方保持されること
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert_eq!(row.label.as_deref(), Some("eq:pythag"));
    assert!(row.numbered);
  }

  #[test]
  fn equation_rejects_column_break() {
    // Arrange — equation 本体の `&`（列区切り）はエラー（複数列は align / matrix を使う）
    let arena = Bump::new();
    let source = r"\begin{equation}a & b\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn equation_rejects_row_break() {
    // Arrange — equation 本体の `\\`（行区切り）はエラー（複数行は align / gather を使う）
    let arena = Bump::new();
    let source = r"\begin{equation}a \\ b\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn equation_rejects_unknown_opt_key() {
    // Arrange — equation は label / numbered のみ許可、未知キーはエラー
    let arena = Bump::new();
    let source = r"\begin{equation}[foo=1]x\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn equation_numbered_false_suppresses_numbering() {
    // Arrange — `[numbered=false]` で無採番（numbered が false）になる
    let arena = Bump::new();
    let source = r"\begin{equation}[numbered=false]x\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — 行は出るが採番対象ではない
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(!row.numbered, "無採番なので numbered は false のはず");
  }

  #[test]
  fn equation_numbered_true_is_explicit_default() {
    // Arrange — `[numbered=true]` 明示は既定（採番あり）と同じ
    let arena = Bump::new();
    let source = r"\begin{equation}[numbered=true]x\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let row = first_row(&result);
    assert!(row.numbered);
  }

  #[test]
  fn equation_numbered_false_with_label_errors() {
    // Arrange — 無採番の式にラベルを付けるとエラー（参照番号が存在しないため）
    let arena = Bump::new();
    let source = r"\begin{equation}[numbered=false][label=eq:x]a\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "equation"));
  }

  #[test]
  fn equation_rejects_notag() {
    // Arrange — equation は行ごと採番でないため \notag は不可（無採番にするなら [numbered=false]）
    let arena = Bump::new();
    let source = r"\begin{equation}a \notag\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotSupported { .. })));
  }

  #[test]
  fn equation_rejects_row_label_marker() {
    // Arrange — equation は行末マーカー `\label{...}` を受け付けない（ラベルは `[label=...]` を使う）
    let arena = Bump::new();
    let source = r"\begin{equation}a \label{eq:x}\end{equation}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
