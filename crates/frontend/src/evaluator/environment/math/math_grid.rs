//! 複数行数式環境の構造分割器と共通ハンドラ
//!
//! 数式環境の本体（`EnvironmentBody`）をトップレベルの行区切り `\\`（[`TokenKind::LineBreak`]）で
//! 行に、各行をトップレベルの列区切り `&`（[`TokenKind::Ampersand`]）でセルに分割し、各セルを
//! [`crate::evaluator::math::evaluate_math_elements`] で `MathNode` 列に評価する。
//!
//! 表環境の `\row` のセル分割（`environment/table.rs` の `extract_row`）と同じ「トップレベルの
//! 区切りトークンで要素列を割る」方式を数式モードへ流用したもの。`equation` のように行・列分割を
//! 許さない環境では、分割トークンの出現を [`EvalError::UnsupportedInMath`] にする。
//!
//! また、`align` / `gather` / `split` / `multiline` の各ハンドラが共有する評価本体 [`evaluate_math_env`]
//! もここに置く（任意引数 `[numbered]` の解釈・グリッド分割・末尾空行除去・採番までを一手に行う）。

use miette::SourceSpan;
use model::{DocNode, MathEnvKind, MathNode, MathRow};

use crate::{
  evaluator::{EvalError, math::evaluate_math_elements},
  span_ext::ToSourceSpan,
  syntax::{
    ast::EnvironmentView,
    green::{GreenElement, GreenNode},
    token::TokenKind,
  },
};

mod markers;
mod numbering;

use markers::{ensure_markers_at_row_end, take_row_label, try_take_row_marker};
pub(crate) use numbering::NumberingMode;
use numbering::{assign_numbering, parse_math_env_opts, trim_trailing_blank_marker_rows};

/// グリッド分割の許可設定（環境種別ごと）
///
/// `equation` は両方 `false`（単一行・単一セル）、`gather` / `multiline` は行のみ、
/// `align` / `split` / `matrix` / `cases` は両方 `true`。
pub(crate) struct GridSpec {
  /// 行区切り `\\` を許可するか
  pub allow_row_breaks: bool,
  /// 列区切り `&` を許可するか
  pub allow_column_breaks: bool,
}

/// グリッド 1 行の評価結果
///
/// `cells` は `&` で分割した各列の `MathNode` 列。`notag_span` はその行に行末マーカー `\notag` が
/// 付いていた場合の `\notag` のソース位置（`None` は採番対象）。行ごと採番の `align` / `gather` では
/// `notag_span.is_some()` の行を無採番にする。`label` / `label_span` は行末マーカー `\label{...}` で
/// 付与された行ラベルとその位置（`None` は参照対象外）。
#[derive(Debug)]
pub(crate) struct GridRow {
  /// 列（`&` 区切り）。各列は数式ノード列
  pub cells: Vec<Vec<MathNode>>,
  /// 行末マーカー `\notag` の位置（`None` は採番する）
  pub notag_span: Option<SourceSpan>,
  /// 行末マーカー `\label{...}` で付与された行ラベル（`None` は参照対象外）
  pub label: Option<String>,
  /// 行末マーカー `\label` のソース位置（重複・無採番診断用。`label` が `Some` なら `Some`）
  pub label_span: Option<SourceSpan>,
}

/// 数式環境本体を行 × 列のグリッドに分割して評価する
///
/// 戻り値は [`GridRow`] のリスト。各行はセル（列）のリストと行末マーカー（`\notag` / `\label`）の
/// 情報を持つ。`equation` のように分割を許さない環境では常に「1 行 1 セル」を返す（本体が空でも
/// 1 行 1 空セル）。
///
/// `row_markers_allowed` が `true`（行ごと採番の `align` / `gather`）のときのみ行末マーカー
/// `\notag`（その行を無採番にする）と `\label{...}`（その行にラベルを付ける）を受理し、行の
/// `notag_span` / `label` を立てる（マーカー自体はセルに残さないため後段の数式評価には現れない）。
///
/// # Errors
///
/// 許可していない区切りトークン（`\\` / `&`）の出現、行末マーカーの不正な使用（非許可環境＝
/// [`EvalError::NotagNotSupported`] / [`EvalError::RowLabelNotSupported`]、行末以外・引数不正・1 行に
/// 複数＝[`EvalError::NotagNotAtRowEnd`] / [`EvalError::RowLabelNotAtRowEnd`]）、セル内の数式評価失敗時に
/// エラーを返す。
pub(crate) fn evaluate_grid(
  source: &str,
  body: &GreenNode,
  spec: &GridSpec,
  row_markers_allowed: bool,
) -> Result<Vec<GridRow>, EvalError> {
  let mut rows: Vec<GridRow> = Vec::new();
  let mut current_row: Vec<Vec<MathNode>> = Vec::new();
  let mut current_cell: Vec<GreenElement> = Vec::new();
  let mut current_notag: Option<SourceSpan> = None;
  let mut current_label: Option<(String, SourceSpan)> = None;

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
          current_row.push(evaluate_math_elements(source, &current_cell)?);
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
          current_row.push(evaluate_math_elements(source, &current_cell)?);
          current_cell.clear();
          let (label, label_span) = take_row_label(&mut current_label);
          rows.push(GridRow {
            cells: std::mem::take(&mut current_row),
            notag_span: current_notag.take(),
            label,
            label_span,
          });
          continue;
        },
        _ => {},
      }
    }

    // 行末マーカー `\notag` / `\label{...}` を検出したら走査ローカル状態へ取り込む
    if try_take_row_marker(child, source, row_markers_allowed, &mut current_notag, &mut current_label)? {
      continue;
    }

    // 行末マーカーの後ろに意味のある要素が来たら行末ではない（末尾空白等のトリビアは許容）
    if !is_trivia_element(child) {
      ensure_markers_at_row_end(current_notag.as_ref(), current_label.as_ref())?;
    }

    current_cell.push(*child);
  }

  // 末尾のセル・行を確定する（行区切りで終わっていなければ最後の行を 1 つ積む）
  current_row.push(evaluate_math_elements(source, &current_cell)?);
  let (label, label_span) = take_row_label(&mut current_label);
  rows.push(GridRow {
    cells: current_row,
    notag_span: current_notag,
    label,
    label_span,
  });
  return Ok(rows);
}

/// `align` / `gather` / `split` / `multiline` の共通評価本体
///
/// 任意引数 `[numbered]`（既定 `true`）を許可し、環境単位採番（`SingleEnv` = `split` / `multiline`）では
/// 加えて `[label=...]` を受理する。本体を `spec` に従って `\\`×`&` のグリッドへ分割（[`evaluate_grid`]）
/// したのち末尾の空行を除去し、`mode` に応じて採番した `DocNode::MathBlock` を返す。`numbered == false`
/// のときは採番を一切行わない（採番ありの環境を無採番にする）。
///
/// ラベルは採番粒度に揃える。`PerRow`（`align` / `gather`）は行末マーカー `\label{...}` をその行の
/// `MathRow::label` に、`SingleEnv`（`split` / `multiline`）は環境の `[label=...]` を `MathBlock::label` に
/// 載せる。いずれも無採番の行/環境にラベルは付けられない（参照番号が無いため [`EvalError::LabelRequiresNumbering`]）。
///
/// # Errors
///
/// 未知の任意引数キー・位置引数の指定、本体のセル評価や許可しない区切りトークンの出現、無採番への
/// ラベル付与・重複ラベル時にエラーを返す。
pub(crate) fn evaluate_math_env(
  view: &EnvironmentView,
  kind: MathEnvKind,
  spec: &GridSpec,
  mode: &NumberingMode,
) -> Result<Vec<DocNode>, EvalError> {
  let (numbered, env_label) = parse_math_env_opts(view, mode)?;

  // 行末マーカー `\notag` / `\label` は行ごと採番（`PerRow`）の環境でのみ意味を持つ
  let row_markers_allowed = matches!(mode, NumberingMode::PerRow);
  let mut grid = match view.body() {
    Some(body_node) => evaluate_grid(view.source(), body_node, spec, row_markers_allowed)?,
    None => Vec::new(),
  };
  trim_trailing_blank_marker_rows(&mut grid)?;

  // `[numbered=false]` で既に全行が無採番なら、行単位の `\notag` は冗長・矛盾
  if !numbered && let Some(span) = grid.iter().find_map(|row| return row.notag_span) {
    return Err(EvalError::NotagWithUnnumberedEnv { span });
  }

  let (rows, env_numbered) = assign_numbering(grid, mode, numbered, view)?;

  // 環境単位ラベルは環境が採番対象の場合のみ持たせる（無採番・空ブロックはダングリングアンカーを避け None）
  let block_label = env_numbered.then_some(env_label).flatten();
  return Ok(vec![DocNode::MathBlock {
    kind,
    rows,
    numbered: env_numbered,
    label: block_label,
    span: view.span(),
  }]);
}

/// 非採番環境（`cases` / `matrix`）の行リストを構築する
///
/// 末尾の空行（行末 `\\` 由来）を除去したうえで、各行を採番なし（`numbered: false`・`label: None`）の
/// [`MathRow`] に変換する。`cases` / `matrix` は番号を持たないため、`CounterName::Equation` を一切
/// 消費しない（採番ありの環境と通し番号を共有しない）。
pub(crate) fn into_unnumbered_rows(mut grid: Vec<GridRow>) -> Vec<MathRow> {
  while grid.last().is_some_and(|row| return is_blank_row(&row.cells)) {
    grid.pop();
  }
  return grid
    .into_iter()
    .map(|row| {
      return MathRow {
        cells: row.cells,
        numbered: false,
        label: None,
        label_span: None,
      };
    })
    .collect();
}

/// 行が空（全セルが空白のみ）かどうかを判定する
///
/// 構造分割器は行末 `\\` のあとに空白テキストだけの行を 1 つ残すため、採番前にその末尾行を
/// 除去する。空セル、または改行・スペースだけの `Text` ノードしか含まない行を空とみなす。
fn is_blank_row(row: &[Vec<MathNode>]) -> bool {
  return row
    .iter()
    .all(|cell| return cell.iter().all(|node| matches!(node, MathNode::Text(t) if t.trim().is_empty())));
}

/// 要素がトリビア（空白・改行・コメント・段落区切り）かどうかを判定する
///
/// 行末マーカー `\notag` の位置検証で、`\notag` の後ろに来てよい無意味な要素（末尾の空白・改行・
/// コメント）を許容するために使う。これら以外のトークン / ノードが `\notag` の後ろに続く場合は
/// 「行末ではない」と判断する。
fn is_trivia_element(child: &GreenElement) -> bool {
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
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use model::MathNode;

  use super::*;
  use crate::{
    evaluator::lookup_env_parse_mode,
    syntax::{SyntaxKind, ast::EnvironmentView, green::GreenElement},
  };

  /// ソースをパースし、最初の `Environment` ノードの本体（math モード構造化済み）を返す
  ///
  /// `equation` は `ParseMode::Math` で登録されているため、本体の `&` / `\\` はトップレベルの
  /// トークンとして現れ、分割器の入力になる。
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

  fn first_env_body<'a>(source: &'a str, arena: &'a Bump) -> &'a GreenNode<'a> {
    let root = crate::syntax::parse(source, arena, lookup_env_parse_mode).unwrap();
    let env = find_env(root).expect("Environment ノードが見つからない");
    return EnvironmentView::new(env, source).body().expect("環境本体あり");
  }

  /// セルのプレーンテキスト（`MathNode::Text` の連結）を取り出すヘルパ
  fn cell_text(cell: &[MathNode]) -> String {
    return cell
      .iter()
      .filter_map(|n| match n {
        MathNode::Text(t) => return Some(t.as_str()),
        _ => return None,
      })
      .collect::<String>()
      .split_whitespace()
      .collect();
  }

  #[test]
  fn splits_rows_and_columns_when_allowed() {
    // Arrange — `a & b \\ c & d` を 2 行 × 2 列に分割する
    let arena = Bump::new();
    let source = r"\begin{equation}a & b \\ c & d\end{equation}";
    let body = first_env_body(source, &arena);
    let spec = GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: true,
    };

    // Act
    let grid = evaluate_grid(source, body, &spec, true).unwrap_or_else(|e| panic!("分割に失敗: {e:?}"));

    // Assert — 2 行、各行 2 セル、内容は a/b/c/d
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
    // Arrange — 区切りなしの本体は許可設定に関わらず 1 行 1 セル
    let arena = Bump::new();
    let source = r"\begin{equation}x + y\end{equation}";
    let body = first_env_body(source, &arena);
    let spec = GridSpec {
      allow_row_breaks: false,
      allow_column_breaks: false,
    };

    // Act
    let grid = evaluate_grid(source, body, &spec, false).unwrap();

    // Assert
    assert_eq!(grid.len(), 1);
    assert_eq!(grid[0].cells.len(), 1);
  }

  #[test]
  fn rejects_column_break_when_not_allowed() {
    // Arrange — 列区切りを許可しない設定で `&` が現れるとエラー
    let arena = Bump::new();
    let source = r"\begin{equation}a & b\end{equation}";
    let body = first_env_body(source, &arena);
    let spec = GridSpec {
      allow_row_breaks: true,
      allow_column_breaks: false,
    };

    // Act
    let result = evaluate_grid(source, body, &spec, false);

    // Assert
    assert!(matches!(result, Err(EvalError::UnsupportedInMath { .. })));
  }
}
