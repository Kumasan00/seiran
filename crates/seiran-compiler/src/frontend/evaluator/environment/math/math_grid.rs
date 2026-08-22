//! 複数行数式環境の構造分割器と共通ハンドラ
//!
//! トップレベルの `\\` と `&` で本体を行とセルに分割する。

use miette::SourceSpan;

use crate::{
  document::{HirBuilder, HirMath, HirMathKind, HirMathRow, HirNode, HirNodeKind, MathEnvKind},
  frontend::{
    evaluator::{EvalError, math::evaluate_math_elements},
    span_ext::ToSourceSpan,
    syntax::{
      ast::EnvironmentView,
      green::{GreenElement, GreenNode},
      token::TokenKind,
    },
  },
};

mod markers;
mod numbering;

pub(crate) use markers::RowLabel;
use markers::{ensure_markers_at_row_end, try_take_row_marker};
pub(crate) use numbering::NumberingMode;
use numbering::{assign_numbering, parse_math_env_opts, trim_trailing_blank_marker_rows};

use crate::document::NodeId;

/// グリッド分割の許可設定（環境種別ごと）
pub(crate) struct GridSpec {
  /// 行区切り `\\` を許可するか
  pub allow_row_breaks: bool,
  /// 列区切り `&` を許可するか
  pub allow_column_breaks: bool,
}

/// グリッド 1 行の評価結果
#[derive(Debug)]
pub(crate) struct GridRow {
  /// この行の HIR ノード ID（セルより先に確保する）
  pub id: NodeId,
  /// 列（`&` 区切り）。各列は数式ノード列
  pub cells: Vec<Vec<HirMath>>,
  /// 行末マーカー `\notag` の位置（`None` は採番する）
  pub notag_span: Option<SourceSpan>,
  /// 行末マーカー `\label{...}` で付与された行ラベル（`None` は参照対象外）
  pub label: Option<RowLabel>,
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
pub(crate) fn evaluate_grid(
  source: &str,
  builder: &HirBuilder,
  body: &GreenNode<'_>,
  spec: &GridSpec,
  row_markers_allowed: bool,
) -> Result<Vec<GridRow>, EvalError> {
  let mut rows: Vec<GridRow> = Vec::new();
  let mut current_row: Vec<Vec<HirMath>> = Vec::new();
  let mut current_cell: Vec<GreenElement<'_>> = Vec::new();
  let mut current_notag: Option<SourceSpan> = None;
  let mut current_label: Option<RowLabel> = None;
  // 行 ID はセルより先に確保する（行の位置は本体全体を覆う span から始め、行区切りで更新する）
  let mut current_row_id = builder.alloc(body.span);

  for child in body.children {
    if let GreenElement::Token(token) = child {
      match token.kind {
        TokenKind::Ampersand => {
          if !spec.allow_column_breaks {
            return Err(EvalError::UnsupportedInMath {
              what: r"&（列区切り）".to_string(),
              span: token.span.to_source_span(),
            });
          }
          // 行末マーカーの後ろに列が続くなら、マーカーは行末になく不正
          ensure_markers_at_row_end(current_notag.as_ref(), current_label.as_ref())?;
          current_row.push(evaluate_math_elements(source, builder, &current_cell)?);
          current_cell.clear();
          continue;
        },
        TokenKind::LineBreak => {
          if !spec.allow_row_breaks {
            return Err(EvalError::UnsupportedInMath {
              what: r"\\（行区切り）".to_string(),
              span: token.span.to_source_span(),
            });
          }
          current_row.push(evaluate_math_elements(source, builder, &current_cell)?);
          current_cell.clear();
          rows.push(GridRow {
            id: current_row_id,
            cells: std::mem::take(&mut current_row),
            notag_span: current_notag.take(),
            label: current_label.take(),
          });
          current_row_id = builder.alloc(token.span);
          continue;
        },
        _ => {},
      }
    }

    // 行末マーカー `\notag` / `\label{...}` を検出したら走査ローカル状態へ取り込む
    if try_take_row_marker(child, source, builder, row_markers_allowed, &mut current_notag, &mut current_label)? {
      continue;
    }

    // 行末マーカーの後ろに意味のある要素が来たら行末ではない（末尾空白等のトリビアは許容）
    if !is_trivia_element(child) {
      ensure_markers_at_row_end(current_notag.as_ref(), current_label.as_ref())?;
    }

    current_cell.push(*child);
  }

  // 末尾のセル・行を確定する（行区切りで終わっていなければ最後の行を 1 つ積む）
  current_row.push(evaluate_math_elements(source, builder, &current_cell)?);
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
/// グリッド分割後に [`NumberingMode`] に応じた採番対象とラベルを構造化する。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価や許可しない区切りトークンの出現、無採番への
/// ラベル付与・重複ラベル時にエラーを返す。
pub(crate) fn evaluate_math_env(
  view: &EnvironmentView<'_>,
  builder: &HirBuilder,
  kind: MathEnvKind,
  spec: &GridSpec,
  mode: &NumberingMode,
) -> Result<Vec<HirNode>, EvalError> {
  let (numbered, env_label) = parse_math_env_opts(view, mode)?;

  // 行末マーカー `\notag` / `\label` は行ごと採番（`PerRow`）の環境でのみ意味を持つ
  let row_markers_allowed = matches!(mode, NumberingMode::PerRow);
  let id = builder.alloc(view.span());
  let mut grid = match view.body() {
    Some(body_node) => evaluate_grid(view.source(), builder, body_node, spec, row_markers_allowed)?,
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
  return Ok(vec![HirNode::new(
    id,
    HirNodeKind::MathBlock {
      kind,
      rows,
      numbered: env_numbered,
      label: block_label,
    },
  )]);
}

/// 非採番環境（`cases` / `matrix`）の行リストを構築する
pub(crate) fn into_unnumbered_rows(mut grid: Vec<GridRow>) -> Vec<HirMathRow> {
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
    document::{HirBuilder, HirMathKind},
    frontend::{
      evaluator::lookup_env_parse_mode,
      syntax,
      syntax::{SyntaxKind, ast::EnvironmentView, green::GreenElement},
    },
    source::SourceId,
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
    let root = syntax::parse(source, arena, lookup_env_parse_mode).unwrap();
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
    let builder = HirBuilder::new(SourceId::new(0));
    let grid = evaluate_grid(source, &builder, body, &spec, true).unwrap_or_else(|e| panic!("分割に失敗: {e:?}"));

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
    let builder = HirBuilder::new(SourceId::new(0));
    let grid = evaluate_grid(source, &builder, body, &spec, false).unwrap();

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
    let builder = HirBuilder::new(SourceId::new(0));
    let result = evaluate_grid(source, &builder, body, &spec, false);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }
}
