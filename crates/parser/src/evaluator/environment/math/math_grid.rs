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
  ast::{CommandView, EnvironmentView, extract_text_content},
  green::{GreenElement, GreenNode},
  token::TokenKind,
};

use crate::evaluator::{
  EvalError, Evaluator,
  math::evaluate_math_elements,
  opt_args::{OptType, collect_environment_opt_args, find_bool, find_string},
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
              span: token.span.into(),
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
              span: token.span.into(),
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

/// 行末マーカー `\label{...}` の蓄積を行の `(label, label_span)` に取り出すヘルパ
///
/// `current_label` を消費し、ラベル文字列とその位置をそれぞれの `Option` に分解して返す。
fn take_row_label(current_label: &mut Option<(String, SourceSpan)>) -> (Option<String>, Option<SourceSpan>) {
  return match current_label.take() {
    Some((label, span)) => (Some(label), Some(span)),
    None => (None, None),
  };
}

/// 立っている行末マーカーの後ろに意味のある要素が続いていないか検証する
///
/// 列区切り `&` や非トリビア要素がマーカーの後に現れたら、そのマーカーは行末になく不正。
/// `\notag` を先に、続いて `\label` を検査して [`EvalError::NotagNotAtRowEnd`] /
/// [`EvalError::RowLabelNotAtRowEnd`] を返す。
fn ensure_markers_at_row_end(
  current_notag: Option<&SourceSpan>,
  current_label: Option<&(String, SourceSpan)>,
) -> Result<(), EvalError> {
  if let Some(span) = current_notag {
    return Err(EvalError::NotagNotAtRowEnd { span: *span });
  }
  if let Some((_, span)) = current_label {
    return Err(EvalError::RowLabelNotAtRowEnd { span: *span });
  }
  return Ok(());
}

/// 行末マーカー `\notag` / `\label{...}` を検出し、走査ローカル状態へ取り込む
///
/// `child` が `\notag` / `\label` の `CommandCall` ノードならそれぞれの検証を行って
/// `current_notag` / `current_label` を立て、`Ok(true)`（呼び出し側は `continue`）を返す。
/// それ以外の要素は `Ok(false)` を返し、呼び出し側はセル要素として扱う。
fn try_take_row_marker(
  child: &GreenElement,
  source: &str,
  row_markers_allowed: bool,
  current_notag: &mut Option<SourceSpan>,
  current_label: &mut Option<(String, SourceSpan)>,
) -> Result<bool, EvalError> {
  let GreenElement::Node(node) = *child else {
    return Ok(false);
  };
  if node.kind != SyntaxKind::CommandCall {
    return Ok(false);
  }
  let view = CommandView::new(node, source);
  let span: SourceSpan = node.span.into();
  match view.name() {
    "notag" => {
      take_notag_marker(&view, span, row_markers_allowed, current_notag)?;
      return Ok(true);
    },
    "label" => {
      take_label_marker(&view, source, span, row_markers_allowed, current_label)?;
      return Ok(true);
    },
    _ => return Ok(false),
  }
}

/// 行末マーカー `\notag` を検証して走査ローカル状態 `current_notag` を立てる
///
/// `row_markers_allowed` が `false` の環境では [`EvalError::NotagNotSupported`]。`\notag` は引数を
/// 取らないため引数付き・任意引数付き、または 1 行に複数現れた場合は [`EvalError::NotagNotAtRowEnd`]。
fn take_notag_marker(
  view: &CommandView,
  span: SourceSpan,
  row_markers_allowed: bool,
  current_notag: &mut Option<SourceSpan>,
) -> Result<(), EvalError> {
  if !row_markers_allowed {
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
  *current_notag = Some(span);
  return Ok(());
}

/// 行末マーカー `\label{...}` を検証してラベル文字列を抽出し、走査ローカル状態 `current_label` を立てる
///
/// `row_markers_allowed` が `false` の環境では [`EvalError::RowLabelNotSupported`]。`\label` は必須引数
/// 1 個（ラベル名）のみで、引数過不足・任意引数付き、または 1 行に複数現れた場合は
/// [`EvalError::RowLabelNotAtRowEnd`]。
fn take_label_marker(
  view: &CommandView,
  source: &str,
  span: SourceSpan,
  row_markers_allowed: bool,
  current_label: &mut Option<(String, SourceSpan)>,
) -> Result<(), EvalError> {
  if !row_markers_allowed {
    return Err(EvalError::RowLabelNotSupported { span });
  }
  // `\label` は必須引数 1 個（ラベル名）のみ。引数過不足・任意引数付きは不正
  if view.args_count() != 1 || view.opt_args_count() > 0 {
    return Err(EvalError::RowLabelNotAtRowEnd { span });
  }
  // 1 行に `\label` は 1 つだけ
  if current_label.is_some() {
    return Err(EvalError::RowLabelNotAtRowEnd { span });
  }
  let first_arg = view.first_arg().ok_or(EvalError::RowLabelNotAtRowEnd { span })?;
  let label = extract_text_content(source, first_arg).trim().to_string();
  *current_label = Some((label, span));
  return Ok(());
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
  evaluator: &mut Evaluator,
  kind: MathEnvKind,
  spec: &GridSpec,
  mode: &NumberingMode,
) -> Result<Vec<DocNode>, EvalError> {
  // 環境単位ラベル `[label=...]` は環境全体に 1 番号を振る `SingleEnv`（split / multiline）でのみ受理する。
  // 行ごと採番（`PerRow` = align / gather）の行単位ラベルは行末マーカー `\label{...}` で指定する。
  let allow_env_label = matches!(mode, NumberingMode::SingleEnv);
  let schema: &[(&str, OptType)] = if allow_env_label {
    &[("label", OptType::String), ("numbered", OptType::Bool)]
  } else {
    &[("numbered", OptType::Bool)]
  };
  let opt_args = collect_environment_opt_args(view, schema)?;
  let numbered = find_bool(&opt_args, "numbered").unwrap_or(true);
  let env_label = find_string(&opt_args, "label");
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
  // 無採番の環境は参照番号を持たないため、環境単位ラベルとの併用を禁じる（equation と同じ規則）
  if !numbered && env_label.is_some() {
    return Err(EvalError::LabelRequiresNumbering {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }

  // 行末マーカー `\notag` / `\label` は行ごと採番（`PerRow`）の環境でのみ意味を持つ
  let row_markers_allowed = matches!(mode, NumberingMode::PerRow);
  let source = view.source();
  let mut grid = match view.body() {
    Some(body_node) => evaluate_grid(source, body_node, spec, row_markers_allowed)?,
    None => Vec::new(),
  };
  // 行末の `\\` は分割器が末尾に空白だけの行を 1 つ生むため、採番前に除去する。
  // 中身が空なのにマーカー（`\notag` / `\label`）だけ付いた末尾行は行末に式がないためエラーにする。
  while let Some(last) = grid.last() {
    if !is_blank_row(&last.cells) {
      break;
    }
    if let Some(span) = last.notag_span {
      return Err(EvalError::NotagNotAtRowEnd { span });
    }
    if let Some(span) = last.label_span {
      return Err(EvalError::RowLabelNotAtRowEnd { span });
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
      .map(|row| -> Result<MathRow, EvalError> {
        // `\notag` 行（`notag_span` あり）はカウンタを消費せず無採番にする
        let numbered_row = numbered && row.notag_span.is_none();
        // 無採番の行に行ラベルは付けられない（参照番号が無いため）
        if let Some(span) = row.label_span
          && !numbered_row
        {
          return Err(EvalError::LabelRequiresNumbering {
            name: view.name().to_string(),
            span,
          });
        }
        let number = if numbered_row {
          Some(evaluator.registry.increment_with_label(
            CounterName::Equation,
            row.label.as_deref(),
            row.label_span.unwrap_or_else(|| view.span().into()),
          )?)
        } else {
          None
        };
        return Ok(MathRow {
          cells: row.cells,
          number,
          label: row.label,
        });
      })
      .collect::<Result<Vec<MathRow>, EvalError>>()?,
    NumberingMode::SingleEnv => {
      // 行は無採番。環境全体に 1 つだけ（空ブロックには採番しない）
      if numbered && !grid.is_empty() {
        env_number = Some(evaluator.registry.increment_with_label(
          CounterName::Equation,
          env_label.as_deref(),
          view.span().into(),
        )?);
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

  // 環境単位ラベルは採番できた場合のみ持たせる（無採番・空ブロックはダングリングアンカーを避け None）
  let block_label = env_number.as_ref().and(env_label);
  return Ok(vec![DocNode::MathBlock {
    kind,
    rows,
    number: env_number,
    label: block_label,
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
