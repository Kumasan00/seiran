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

use document::{DocNode, MathEnvKind, MathNode, MathRow};
use miette::SourceSpan;
use read_style::CounterName;
use syntax::{
  SyntaxKind,
  ast::{CommandView, EnvironmentView},
  green::{GreenElement, GreenNode},
  token::TokenKind,
};

use crate::evaluator::{
  EvalError, Evaluator,
  math::evaluate_math_elements,
  opt_args::{OptType, collect_environment_opt_args, find_bool},
};

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
/// `notag_span.is_some()` の行を無採番にする。
#[derive(Debug)]
pub(crate) struct GridRow {
  /// 列（`&` 区切り）。各列は数式ノード列
  pub cells: Vec<Vec<MathNode>>,
  /// 行末マーカー `\notag` の位置（`None` は採番する）
  pub notag_span: Option<SourceSpan>,
}

/// 数式環境本体を行 × 列のグリッドに分割して評価する
///
/// 戻り値は [`GridRow`] のリスト。各行はセル（列）のリストと行末マーカー `\notag` の有無
/// （`notag_span`）を持つ。`equation` のように分割を許さない環境では常に「1 行 1 セル」を返す
/// （本体が空でも 1 行 1 空セル）。
///
/// `notag_allowed` が `true`（行ごと採番の `align` / `gather`）のときのみ行末の `\notag` を受理し、
/// その行の `notag_span` を立てる（マーカー自体はセルに残さないため後段の数式評価には現れない）。
///
/// # Errors
///
/// 許可していない区切りトークン（`\\` / `&`）の出現、`\notag` の不正な使用（非許可環境＝
/// [`EvalError::NotagNotSupported`]、行末以外・引数付き・1 行に複数＝[`EvalError::NotagNotAtRowEnd`]）、
/// セル内の数式評価失敗時にエラーを返す。
pub(crate) fn evaluate_grid(
  source: &str,
  body: &GreenNode,
  spec: &GridSpec,
  notag_allowed: bool,
) -> Result<Vec<GridRow>, EvalError> {
  let mut rows: Vec<GridRow> = Vec::new();
  let mut current_row: Vec<Vec<MathNode>> = Vec::new();
  let mut current_cell: Vec<GreenElement> = Vec::new();
  let mut current_notag: Option<SourceSpan> = None;

  for child in body.children {
    if let GreenElement::Token(token) = child {
      match token.kind {
        TokenKind::Ampersand => {
          if !spec.allow_column_breaks {
            return Err(EvalError::UnsupportedInMath {
              what: r"&（列区切り）".to_string(),
              span: token.span.into(),
            });
          }
          // `\notag` の後ろに列が続くなら、`\notag` は行末になく不正
          if let Some(span) = current_notag {
            return Err(EvalError::NotagNotAtRowEnd { span });
          }
          current_row.push(evaluate_math_elements(source, &current_cell)?);
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
          current_row.push(evaluate_math_elements(source, &current_cell)?);
          current_cell.clear();
          rows.push(GridRow {
            cells: std::mem::take(&mut current_row),
            notag_span: current_notag.take(),
          });
          continue;
        },
        _ => {},
      }
    }

    // 行末マーカー `\notag` の検出（CommandCall ノード）
    if let GreenElement::Node(node) = *child
      && node.kind == SyntaxKind::CommandCall
    {
      let view = CommandView::new(node, source);
      if view.name() == "notag" {
        let span: SourceSpan = node.span.into();
        if !notag_allowed {
          return Err(EvalError::NotagNotSupported { span });
        }
        // `\notag` は引数を取らない（`\notag{...}` が後続の中身を飲み込むのを防ぐ）
        if !view.args_is_empty() || view.opt_args_count() > 0 {
          return Err(EvalError::NotagNotAtRowEnd { span });
        }
        // 1 行に `\notag` は 1 つだけ
        if current_notag.is_some() {
          return Err(EvalError::NotagNotAtRowEnd { span });
        }
        current_notag = Some(span);
        continue;
      }
    }

    // `\notag` の後ろに意味のある要素が来たら行末ではない（末尾空白等のトリビアは許容）
    if let Some(span) = current_notag
      && !is_trivia_element(child)
    {
      return Err(EvalError::NotagNotAtRowEnd { span });
    }

    current_cell.push(*child);
  }

  // 末尾のセル・行を確定する（行区切りで終わっていなければ最後の行を 1 つ積む）
  current_row.push(evaluate_math_elements(source, &current_cell)?);
  rows.push(GridRow {
    cells: current_row,
    notag_span: current_notag,
  });
  return Ok(rows);
}

/// 採番の粒度
///
/// 複数行数式環境が `CounterName::Equation` をどの単位で消費するかを表す。
pub(crate) enum NumberingMode {
  /// 各行に 1 つ採番する（`align` / `gather`）。番号は `MathRow::number` に入る
  PerRow,
  /// 環境全体に 1 つだけ採番する（`split` / `multiline`）。番号は `DocNode::MathBlock::number` に入り
  /// `layout` 段が縦中央へ配置する
  SingleEnv,
}

/// `align` / `gather` / `split` / `multiline` の共通評価本体
///
/// 任意引数 `[numbered]`（既定 `true`）のみを許可し、本体を `spec` に従って `\\`×`&` のグリッドへ
/// 分割（[`evaluate_grid`]）したのち末尾の空行を除去し、`mode` に応じて採番した `DocNode::MathBlock`
/// を返す。`numbered == false` のときは採番を一切行わない（採番ありの環境を無採番にする）。
///
/// # Errors
///
/// 未知の任意引数キー（`numbered` 以外）・位置引数の指定、本体のセル評価や許可しない区切りトークンの
/// 出現時にエラーを返す。
pub(crate) fn evaluate_math_env(
  view: &EnvironmentView,
  evaluator: &mut Evaluator,
  kind: MathEnvKind,
  spec: &GridSpec,
  mode: &NumberingMode,
) -> Result<Vec<DocNode>, EvalError> {
  // 許可する任意引数は [numbered] のみ。既定は採番あり
  let opt_args = collect_environment_opt_args(view, &[("numbered", OptType::Bool)])?;
  let numbered = find_bool(&opt_args, "numbered").unwrap_or(true);
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }

  // 行末マーカー `\notag` は行ごと採番（`PerRow`）の環境でのみ意味を持つ
  let notag_allowed = matches!(mode, NumberingMode::PerRow);
  let source = view.source();
  let mut grid = match view.body() {
    Some(body_node) => evaluate_grid(source, body_node, spec, notag_allowed)?,
    None => Vec::new(),
  };
  // 行末の `\\` は分割器が末尾に空白だけの行を 1 つ生むため、採番前に除去する。
  // 中身が空なのに `\notag` だけ付いた末尾行（例 `a \\ \notag`）は行末に式がないためエラーにする。
  while let Some(last) = grid.last() {
    if !is_blank_row(&last.cells) {
      break;
    }
    if let Some(span) = last.notag_span {
      return Err(EvalError::NotagNotAtRowEnd { span });
    }
    grid.pop();
  }

  // `[numbered=false]` で既に全行が無採番なら、行単位の `\notag` は冗長・矛盾
  if !numbered && let Some(span) = grid.iter().find_map(|row| row.notag_span) {
    return Err(EvalError::NotagWithUnnumberedEnv { span });
  }

  let mut env_number = None;
  let rows: Vec<MathRow> = match mode {
    NumberingMode::PerRow => grid
      .into_iter()
      .map(|row| {
        // `\notag` 行（`notag_span` あり）はカウンタを消費せず無採番にする
        let number =
          (numbered && row.notag_span.is_none()).then(|| evaluator.registry.increment(CounterName::Equation));
        return MathRow {
          cells: row.cells,
          number,
          label: None,
        };
      })
      .collect(),
    NumberingMode::SingleEnv => {
      // 行は無採番。環境全体に 1 つだけ（空ブロックには採番しない）
      if numbered && !grid.is_empty() {
        env_number = Some(evaluator.registry.increment(CounterName::Equation));
      }
      grid
        .into_iter()
        .map(|row| MathRow {
          cells: row.cells,
          number: None,
          label: None,
        })
        .collect()
    },
  };

  return Ok(vec![DocNode::MathBlock {
    kind,
    rows,
    number: env_number,
  }]);
}

/// 非採番環境（`cases` / `matrix`）の行リストを構築する
///
/// 末尾の空行（行末 `\\` 由来）を除去したうえで、各行を採番なし（`number` / `label` ともに `None`）の
/// [`MathRow`] に変換する。`cases` / `matrix` は番号を持たないため、`CounterName::Equation` を一切
/// 消費しない（採番ありの環境と通し番号を共有しない）。
pub(crate) fn into_unnumbered_rows(mut grid: Vec<GridRow>) -> Vec<MathRow> {
  while grid.last().is_some_and(|row| is_blank_row(&row.cells)) {
    grid.pop();
  }
  return grid
    .into_iter()
    .map(|row| MathRow {
      cells: row.cells,
      number: None,
      label: None,
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
    .all(|cell| cell.iter().all(|node| matches!(node, MathNode::Text(t) if t.trim().is_empty())));
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
  use document::MathNode;
  use syntax::{SyntaxKind, ast::EnvironmentView, green::GreenElement};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

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
    let root = syntax::parse(source, arena, lookup_env_parse_mode).unwrap();
    let env = find_env(root).expect("Environment ノードが見つからない");
    return EnvironmentView::new(env, source).body().expect("環境本体あり");
  }

  /// セルのプレーンテキスト（`MathNode::Text` の連結）を取り出すヘルパ
  fn cell_text(cell: &[MathNode]) -> String {
    return cell
      .iter()
      .filter_map(|n| match n {
        MathNode::Text(t) => Some(t.as_str()),
        _ => None,
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
