//! 複数行数式環境の構造分割器と共通ハンドラ
//!
//! トップレベルの `\\` と `&` で本体を行とセルに分割する。

use miette::SourceSpan;

use crate::{
  document::{GridLayout, HirMath, HirMathBlock, HirMathKind, HirMathRow, HirNode, HirNodeKind, MathEnvKind},
  frontend::{
    evaluator::{EvalContext, EvalError, math::evaluate_math_elements},
    syntax::{
      green::{GreenElement, GreenNode},
      token::TokenKind,
      view::EnvironmentView,
    },
  },
};

mod markers;
mod numbering;

use markers::{RowLabel, ensure_markers_at_row_end, try_take_row_marker};
pub(in crate::frontend::evaluator::environment) use numbering::NumberingMode;
use numbering::{assign_numbering, parse_math_env_opts, trim_trailing_blank_marker_rows};

use crate::document::NodeId;

/// グリッド分割の許可設定
///
/// 行・列に分割する数式環境（`MathGrid`）は [`GridSpec::for_layout`] でセル配置から導出し、
/// それ以外の数式環境（`equation` / `cases` / `matrix`）は呼び出し側が値を直書きする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GridSpec {
  /// 行区切り `\\` を許可するか
  pub allow_row_breaks: bool,
  /// 列区切り `&` を許可するか
  pub allow_column_breaks: bool,
}

impl GridSpec {
  /// グリッド環境のセル配置から区切りの許可を導出する
  ///
  /// 行区切り `\\` は常に許可し、列区切り `&` は列を持つ配置（[`GridLayout::Aligned`]）だけが受理する。
  const fn for_layout(layout: GridLayout) -> Self {
    let allow_column_breaks = match layout {
      GridLayout::Aligned => true,
      GridLayout::Centered | GridLayout::Staircase => false,
    };
    return Self {
      allow_row_breaks: true,
      allow_column_breaks,
    };
  }
}

/// グリッド 1 行の評価結果
#[derive(Debug)]
pub(super) struct GridRow {
  /// この行の HIR ノード ID（セルより先に確保する）
  pub id: NodeId,
  /// 列（`&` 区切り）。各列は数式ノード列
  pub cells: Vec<Vec<HirMath>>,
  /// 行末マーカー `\notag` の位置（`None` は採番する）
  notag_span: Option<SourceSpan>,
  /// 行末マーカー `\label{...}` で付与された行ラベル（`None` は参照対象外）
  label: Option<RowLabel>,
}

/// 数式環境本体を行 × 列のグリッドに分割して評価する
///
/// 行末マーカーは行のメタデータへ移し、セルには残さない。
///
/// # Errors
///
/// 許可していない区切りトークン（`\\` / `&`）の出現、行末マーカーの不正な使用（非許可環境＝
/// [`EvalError::NotagNotSupported`] / [`EvalError::RowLabelNotSupported`]、行末以外・引数不正・1 行に
/// 複数＝[`EvalError::NotagNotAtRowEnd`] / [`EvalError::RowLabelNotAtRowEnd`]）、セル内の数式評価失敗時に
/// エラーを返す。
pub(super) fn evaluate_grid(
  source: &str,
  ctx: &EvalContext<'_>,
  body: &GreenNode<'_>,
  spec: GridSpec,
  row_markers_allowed: bool,
) -> Result<Vec<GridRow>, EvalError> {
  let mut rows: Vec<GridRow> = Vec::new();
  let mut current_row: Vec<Vec<HirMath>> = Vec::new();
  let mut current_cell: Vec<GreenElement<'_>> = Vec::new();
  let mut current_notag: Option<SourceSpan> = None;
  let mut current_label: Option<RowLabel> = None;
  // 行 ID はセルより先に確保する（行の位置は本体全体を覆う span から始め、行区切りで更新する）
  let mut current_row_id = ctx.alloc(body.span);

  for child in body.children {
    if let GreenElement::Token(token) = child {
      match token.kind {
        TokenKind::Ampersand => {
          if !spec.allow_column_breaks {
            return Err(EvalError::UnsupportedInMath {
              what: "&（列区切り）".to_string(),
              span: token.span.into(),
            });
          }
          // 行末マーカーの後ろに列が続くなら、マーカーは行末になく不正
          ensure_markers_at_row_end(current_notag.as_ref(), current_label.as_ref())?;
          current_row.push(evaluate_math_elements(source, ctx, &current_cell)?);
          current_cell.clear();
          continue;
        },
        TokenKind::LineBreak => {
          if !spec.allow_row_breaks {
            return Err(EvalError::UnsupportedInMath {
              what: r"\\（行区切り）".to_string(),
              span: token.span.into(),
            });
          }
          current_row.push(evaluate_math_elements(source, ctx, &current_cell)?);
          current_cell.clear();
          rows.push(GridRow {
            id: current_row_id,
            cells: std::mem::take(&mut current_row),
            notag_span: current_notag.take(),
            label: current_label.take(),
          });
          current_row_id = ctx.alloc(token.span);
          continue;
        },
        _ => {},
      }
    }

    // 行末マーカー `\notag` / `\label{...}` を検出したら走査ローカル状態へ取り込む
    if try_take_row_marker(child, source, ctx, row_markers_allowed, &mut current_notag, &mut current_label)? {
      continue;
    }

    // 行末マーカーの後ろに意味のある要素が来たら行末ではない（末尾空白等のトリビアは許容）
    if !is_trivia_element(child) {
      ensure_markers_at_row_end(current_notag.as_ref(), current_label.as_ref())?;
    }

    current_cell.push(*child);
  }

  // 末尾のセル・行を確定する（行区切りで終わっていなければ最後の行を 1 つ積む）
  current_row.push(evaluate_math_elements(source, ctx, &current_cell)?);
  rows.push(GridRow {
    id: current_row_id,
    cells: current_row,
    notag_span: current_notag,
    label: current_label.take(),
  });
  return Ok(rows);
}

/// `align` / `gather` / `split` / `multiline` の共通評価本体
///
/// セル配置から区切りの許可を導出してグリッド分割し、[`NumberingMode`] に応じた採番対象とラベルを
/// 構造化する。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価や許可しない区切りトークンの出現、無採番への
/// ラベル付与・重複ラベル時にエラーを返す。
pub(in crate::frontend::evaluator::environment) fn evaluate_math_env(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
  layout: GridLayout,
  mode: NumberingMode,
) -> Result<HirNode, EvalError> {
  let (numbered, env_label) = parse_math_env_opts(view, mode)?;

  // 行末マーカー `\notag` / `\label` は行ごと採番（`PerRow`）の環境でのみ意味を持つ
  let row_markers_allowed = matches!(mode, NumberingMode::PerRow);
  let id = ctx.alloc(view.span());
  let mut grid = match view.body() {
    Some(body_node) => evaluate_grid(view.source(), ctx, body_node, GridSpec::for_layout(layout), row_markers_allowed)?,
    None => Vec::new(),
  };
  trim_trailing_blank_marker_rows(&mut grid)?;

  // 全行が無採番なら、行単位の `\notag` は矛盾する。
  if !numbered && let Some(span) = grid.iter().find_map(|row| return row.notag_span) {
    return Err(EvalError::NotagWithUnnumberedEnv { span });
  }

  let (rows, env_numbered) = assign_numbering(grid, mode, numbered, view)?;

  // 無採番・空ブロックにダングリングアンカーを残さない。
  let block_label = env_numbered.then_some(env_label).flatten();
  return Ok(HirNode::new(
    id,
    HirNodeKind::MathBlock(HirMathBlock {
      kind: MathEnvKind::Grid(layout),
      rows,
      numbered: env_numbered,
      label: block_label,
    }),
  ));
}

/// 非採番環境（`cases` / `matrix`）の行リストを構築する
pub(super) fn into_unnumbered_rows(mut grid: Vec<GridRow>) -> Vec<HirMathRow> {
  while grid.last().is_some_and(|row| return is_blank_row(&row.cells)) {
    grid.pop();
  }
  return grid
    .into_iter()
    .map(|row| {
      return HirMathRow {
        id: row.id,
        cells: row.cells,
        numbered: false,
        label: None,
        label_site: None,
      };
    })
    .collect();
}

/// 行が空（全セルが空白のみ）かどうかを判定する
fn is_blank_row(row: &[Vec<HirMath>]) -> bool {
  return row.iter().all(|cell| {
    return cell.iter().all(|node| matches!(&node.kind, HirMathKind::Text(t) if t.trim().is_empty()));
  });
}

/// 要素がトリビア（空白・改行・コメント・段落区切り）かどうかを判定する
fn is_trivia_element(child: &GreenElement<'_>) -> bool {
  return matches!(
    child,
    GreenElement::Token(token)
      if matches!(
        token.kind,
        TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment | TokenKind::ParagraphBreak
      )
  );
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::HirMathKind,
    frontend::{
      evaluator::{self, mode_resolver, test_support},
      syntax,
      syntax::{SyntaxKind, green::GreenElement, view::EnvironmentView},
      // `crate::frontend::evaluator::test_support`（上の use で束縛済み）と名前が衝突するため、
      // `crate::frontend::test_support` は関数を直接 import する（型・モジュールではなく関数の
      // 直接 import は「出自が自明な慣用」の例外に当たる）。
      test_support::eval_context_for_test,
    },
  };

  /// 緑ツリーを再帰的に走査して最初の `Environment` ノードを返す
  fn find_env<'a>(node: &'a GreenNode<'a>) -> Option<&'a GreenNode<'a>> {
    for child in node.children {
      if let GreenElement::Node(n) = child {
        if n.kind == SyntaxKind::Environment {
          return Some(n);
        }
        if let Some(found) = find_env(n) {
          return Some(found);
        }
      }
    }
    return None;
  }

  /// ソースをパースし、最初の数式環境の本体を返す
  fn first_env_body<'a>(source: &'a str, arena: &'a Bump) -> &'a GreenNode<'a> {
    let root = syntax::parse(source, arena, mode_resolver()).unwrap();
    let env = find_env(root).expect("Environment ノードが見つからない");
    return EnvironmentView::new(env, source).body().expect("環境本体あり");
  }

  /// セルのプレーンテキスト（`HirMathKind::Text` の連結）を取り出すヘルパ
  fn cell_text(cell: &[HirMath]) -> String {
    return cell
      .iter()
      .filter_map(|n| match &n.kind {
        HirMathKind::Text(t) => return Some(t.as_str()),
        _ => return None,
      })
      .collect::<String>()
      .split_whitespace()
      .collect();
  }

  #[test]
  fn splits_rows_and_columns_when_allowed() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}a & b \\ c & d\end{equation}";
    let body = first_env_body(source, &arena);
    let spec = GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: true,
    };

    // Act
    let ctx = eval_context_for_test();
    let grid = evaluate_grid(source, &ctx, body, spec, true).unwrap_or_else(|e| panic!("分割に失敗: {e:?}"));

    // Assert
    assert_eq!(grid.len(), 2, "2 行に分割される: {grid:?}");
    assert_eq!(grid[0].cells.len(), 2);
    assert_eq!(grid[1].cells.len(), 2);
    assert_eq!(cell_text(&grid[0].cells[0]), "a");
    assert_eq!(cell_text(&grid[0].cells[1]), "b");
    assert_eq!(cell_text(&grid[1].cells[0]), "c");
    assert_eq!(cell_text(&grid[1].cells[1]), "d");
  }

  #[test]
  fn single_cell_when_no_breaks_present() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}x + y\end{equation}";
    let body = first_env_body(source, &arena);
    let spec = GridSpec {
      allow_row_breaks: false,
      allow_column_breaks: false,
    };

    // Act
    let ctx = eval_context_for_test();
    let grid = evaluate_grid(source, &ctx, body, spec, false).unwrap();

    // Assert
    assert_eq!(grid.len(), 1);
    assert_eq!(grid[0].cells.len(), 1);
  }

  #[test]
  fn rejects_column_break_when_not_allowed() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{equation}a & b\end{equation}";
    let body = first_env_body(source, &arena);
    let spec = GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: false,
    };

    // Act
    let ctx = eval_context_for_test();
    let result = evaluate_grid(source, &ctx, body, spec, false);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  /// 結果の最初の `HirNodeKind::MathBlock`（`align` ＝ `Grid(Aligned)`）の行スライスを取り出すヘルパ
  fn align_rows_of(result: &[HirNode]) -> &[HirMathRow] {
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.kind, MathEnvKind::Grid(GridLayout::Aligned), "align は Grid(Aligned)");
    return &math.rows;
  }

  #[test]
  fn align_splits_rows_and_columns_and_numbers_each_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let rows = align_rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].cells.len(), 2, "行 0 は 2 列");
    assert_eq!(rows[1].cells.len(), 2, "行 1 は 2 列");
    assert!(rows[0].numbered);
    assert!(rows[1].numbered);
  }

  #[test]
  fn align_single_row_is_numbered() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}x &= y\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    assert!(rows[0].numbered);
  }

  #[test]
  fn align_drops_trailing_blank_row_from_trailing_break() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{align}a &= b \\\\\n\\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert_eq!(rows.len(), 1, "末尾の空行が除去される: {rows:?}");
    assert!(rows[0].numbered);
  }

  #[test]
  fn align_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| return !r.numbered), "無採番のはず: {rows:?}");
  }

  #[test]
  fn align_cell_content_is_evaluated() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}x^{2} &= y\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert!(
      rows[0].cells[0].iter().any(|n| matches!(n.kind, HirMathKind::Superscript(_))),
      "左セルに Superscript ノードが含まれるべき: {:?}",
      rows[0].cells[0]
    );
  }

  #[test]
  fn align_rejects_env_level_label_opt_arg() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[label=eq:foo]a &= b\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn align_row_label_captures_label_and_keeps_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \label{eq:foo} \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].label.as_deref(), Some("eq:foo"));
    assert!(rows[0].numbered);
    assert!(rows[1].label.is_none(), "2 行目はラベルなし: {:?}", rows[1].label);
    assert!(rows[1].numbered);
  }

  #[test]
  fn align_row_label_on_notag_row_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \notag \label{eq:x}\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "align"));
  }

  #[test]
  fn align_row_label_with_numbered_false_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \label{eq:x}\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "align"));
  }

  #[test]
  fn align_row_label_not_at_row_end_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a \label{eq:x} &= b\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotAtRowEnd { .. })));
  }

  #[test]
  fn align_duplicate_row_label_is_structured_without_error() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \label{eq:x} \\ c &= d \label{eq:x}\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert_eq!(rows[0].label.as_deref(), Some("eq:x"));
    assert_eq!(rows[1].label.as_deref(), Some("eq:x"));
  }

  #[test]
  fn align_notag_suppresses_single_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a &= b \\ c &= d \notag \\ e &= f\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = align_rows_of(&result);
    assert_eq!(rows.len(), 3, "3 行に分割される: {rows:?}");
    assert!(rows[0].numbered);
    assert!(!rows[1].numbered, "\\notag 行は無採番のはず");
    assert!(rows[2].numbered);
  }

  #[test]
  fn align_notag_not_at_row_end_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}a \notag &= b\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotAtRowEnd { .. })));
  }

  #[test]
  fn align_notag_with_numbered_false_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{align}[numbered=false]a &= b \notag \\ c &= d\end{align}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagWithUnnumberedEnv { .. })));
  }

  fn gather_rows_of(result: &[HirNode]) -> &[HirMathRow] {
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.kind, MathEnvKind::Grid(GridLayout::Centered), "gather は Grid(Centered)");
    return &math.rows;
  }

  #[test]
  fn gather_splits_rows_each_single_cell_and_numbers_each_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \\ c = d\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = gather_rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 1), "各行 1 セル: {rows:?}");
    assert!(rows.iter().all(|r| return r.numbered));
  }

  #[test]
  fn gather_rejects_column_break() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a & b\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn gather_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}[numbered=false]a = b \\ c = d\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = gather_rows_of(&result);
    assert!(rows.iter().all(|r| return !r.numbered), "無採番のはず: {rows:?}");
  }

  #[test]
  fn gather_notag_suppresses_single_row() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \\ c = d \notag \\ e = f\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = gather_rows_of(&result);
    assert_eq!(rows.len(), 3, "3 行に分割される: {rows:?}");
    assert!(rows[0].numbered);
    assert!(!rows[1].numbered, "\\notag 行は無採番のはず");
    assert!(rows[2].numbered);
  }

  #[test]
  fn gather_notag_not_at_row_end_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a \notag = b\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::NotagNotAtRowEnd { .. })));
  }

  #[test]
  fn gather_row_label_captures_label_and_keeps_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{gather}a = b \label{eq:g} \\ c = d\end{gather}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let rows = gather_rows_of(&result);
    assert_eq!(rows.len(), 2, "2 行に分割される: {rows:?}");
    assert_eq!(rows[0].label.as_deref(), Some("eq:g"));
    assert!(rows[0].numbered);
    assert!(rows[1].label.is_none(), "2 行目はラベルなし: {:?}", rows[1].label);
    assert!(rows[1].numbered);
  }

  /// 最初の `HirNodeKind::MathBlock`（`split` ＝ `Grid(Aligned)`）を分解して (`rows`, `numbered`) を返す
  fn split_block_of(result: &[HirNode]) -> (&[HirMathRow], bool) {
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.kind, MathEnvKind::Grid(GridLayout::Aligned), "split は Grid(Aligned)");
    return (&math.rows, math.numbered);
  }

  #[test]
  fn split_aligns_columns_and_numbers_whole_env_once() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \\ &= c\end{split}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (rows, numbered) = split_block_of(&result);
    assert_eq!(rows.len(), 2, "2 行: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "行は無採番: {rows:?}");
    assert!(numbered, "環境全体は採番対象");
  }

  #[test]
  fn split_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false]a &= b \\ &= c\end{split}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (rows, numbered) = split_block_of(&result);
    assert!(rows.iter().all(|r| return !r.numbered));
    assert!(!numbered, "無採番のはず");
  }

  #[test]
  fn split_with_label_captures_block_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}[label=eq:s]a &= b \\ &= c\end{split}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.label.as_deref(), Some("eq:s"), "環境単位ラベルが付く");
    assert!(math.numbered, "環境全体は採番対象");
    assert!(math.rows.iter().all(|r| return !r.numbered), "行は無採番: {:?}", math.rows);
  }

  #[test]
  fn split_numbered_false_with_label_errors() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}[numbered=false, label=eq:s]a &= b\end{split}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::LabelRequiresNumbering { ref name, .. }) if name == "split"));
  }

  #[test]
  fn split_rejects_row_label_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{split}a &= b \label{eq:s}\end{split}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }

  fn multiline_block_of(result: &[HirNode]) -> (&[HirMathRow], bool) {
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.kind, MathEnvKind::Grid(GridLayout::Staircase), "multiline は Grid(Staircase)");
    return (&math.rows, math.numbered);
  }

  #[test]
  fn multiline_splits_rows_single_cell_and_numbers_whole_env_once() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}a + b \\ + c + d \\ + e\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (rows, numbered) = multiline_block_of(&result);
    assert_eq!(rows.len(), 3, "3 行: {rows:?}");
    assert!(rows.iter().all(|r| return r.cells.len() == 1), "各行 1 セル: {rows:?}");
    assert!(rows.iter().all(|r| return !r.numbered), "行は無採番: {rows:?}");
    assert!(numbered);
  }

  #[test]
  fn multiline_rejects_column_break() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}a & b\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }

  #[test]
  fn multiline_numbered_false_suppresses_numbering() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}[numbered=false]a + b \\ + c\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let (_, numbered) = multiline_block_of(&result);
    assert!(!numbered, "無採番のはず");
  }

  #[test]
  fn multiline_with_label_captures_block_label() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}[label=eq:m]a + b \\ + c\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // Assert
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    assert_eq!(math.label.as_deref(), Some("eq:m"), "環境単位ラベルが付く");
    assert!(math.numbered, "環境全体は採番対象");
  }

  #[test]
  fn multiline_rejects_row_label_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{multiline}a + b \label{eq:m} \\ + c\end{multiline}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluator::evaluate_children_to_hir(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::RowLabelNotSupported { .. })));
  }
}
