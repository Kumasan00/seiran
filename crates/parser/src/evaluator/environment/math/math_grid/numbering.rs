//! 複数行数式環境の採番判定（任意引数の解釈と `MathRow` への割当）
//!
//! 任意引数 `[numbered]` / `[label=...]` の解釈、末尾空行の除去、採番粒度（[`NumberingMode`]）に
//! 応じた `MathRow::numbered` / ラベルの割当を担う。実際の発番（`CounterName::Equation` の消費）は
//! `lowering` 層が担うため、ここでは「採番対象かどうか」の構造化のみを行う。

use document::MathRow;
use syntax::ast::EnvironmentView;

use super::{GridRow, is_blank_row};
use crate::evaluator::{
  EvalError,
  opt_args::{OptType, collect_environment_opt_args, find_bool, find_string},
};

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

/// 数式環境の任意引数 `[numbered]` / `[label=...]` を解析・検証する
///
/// `[numbered]`（既定 `true`）はすべての環境で受理する。環境単位ラベル `[label=...]` は環境全体へ 1 番号を
/// 振る `SingleEnv`（split / multiline）でのみ受理する（行ごと採番 `PerRow` = align / gather の行単位ラベルは
/// 行末マーカー `\label{...}` で指定するため、ここでは受理しない）。返り値は `(numbered, 環境単位ラベル)`。
///
/// # Errors
///
/// 未知の任意引数キー・不正な値、位置引数の指定（[`EvalError::ExtraEnvironmentArgument`]）、無採番環境への
/// 環境単位ラベル付与（[`EvalError::LabelRequiresNumbering`]）でエラーを返す。
pub(super) fn parse_math_env_opts(
  view: &EnvironmentView,
  mode: &NumberingMode,
) -> Result<(bool, Option<String>), EvalError> {
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
  return Ok((numbered, env_label));
}

/// グリッド末尾の空白行を除去し、マーカーだけ残る不正な末尾行を検出する
///
/// 行末の `\\` は分割器が末尾に空白だけの行を 1 つ生むため、採番前に除去する。中身が空なのにマーカー
/// （`\notag` / `\label`）だけ付いた末尾行は、行末に式が無いためエラーにする。
///
/// # Errors
///
/// 末尾の空白行に `\notag` が残る場合は [`EvalError::NotagNotAtRowEnd`]、`\label` が残る場合は
/// [`EvalError::RowLabelNotAtRowEnd`] を返す。
pub(super) fn trim_trailing_blank_marker_rows(grid: &mut Vec<GridRow>) -> Result<(), EvalError> {
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
  return Ok(());
}

/// グリッドを採番粒度（`mode`）に応じて [`MathRow`] 列へ変換する
///
/// `PerRow`（align / gather）は `\notag` の付いていない行を採番対象（`numbered: true`）にする。
/// `SingleEnv`（split / multiline）は各行を常に無採番にし、環境全体の採番要否（`numbered &&
/// !grid.is_empty()`、空ブロックには採番しない）を呼び出し側（`evaluate_math_env`）へ返す。
/// `numbered == false` のときはいずれも採番しない。実際の発番（`CounterName::Equation` の消費）は
/// `lowering` 層が担うため、ここでは行ごとの `numbered` フラグとラベルの構造化のみを行う。
///
/// # Errors
///
/// 無採番の行への行ラベル付与時に [`EvalError::LabelRequiresNumbering`] を返す。
pub(super) fn assign_numbering(
  grid: Vec<GridRow>,
  mode: &NumberingMode,
  numbered: bool,
  view: &EnvironmentView,
) -> Result<(Vec<MathRow>, bool), EvalError> {
  let mut env_numbered = false;
  let rows: Vec<MathRow> = match mode {
    NumberingMode::PerRow => grid
      .into_iter()
      .map(|row| -> Result<MathRow, EvalError> {
        // `\notag` 行（`notag_span` あり）は無採番にする
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
        return Ok(MathRow {
          cells: row.cells,
          numbered: numbered_row,
          label: row.label,
          label_span: row.label_span,
        });
      })
      .collect::<Result<Vec<MathRow>, EvalError>>()?,
    NumberingMode::SingleEnv => {
      // 行は常に無採番。環境全体の採番要否（空ブロックには採番しない）を返す
      env_numbered = numbered && !grid.is_empty();
      grid
        .into_iter()
        .map(|row| MathRow {
          cells: row.cells,
          numbered: false,
          label: None,
          label_span: None,
        })
        .collect()
    },
  };
  return Ok((rows, env_numbered));
}
