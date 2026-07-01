//! 複数行数式環境の採番（`CounterName::Equation` の消費と `MathRow` への割当）
//!
//! 任意引数 `[numbered]` / `[label=...]` の解釈、末尾空行の除去、採番粒度（[`NumberingMode`]）に
//! 応じた `CounterName::Equation` の消費を担う。

use document::MathRow;
use read_style::CounterName;
use syntax::ast::EnvironmentView;

use super::{GridRow, is_blank_row};
use crate::evaluator::{
  EvalError, Evaluator,
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

/// グリッドを採番粒度（`mode`）に応じて [`MathRow`] 列へ変換し、`CounterName::Equation` を消費する
///
/// `PerRow`（align / gather）は採番行ごとにカウンタを 1 回消費し、`\notag` 行はカウンタを消費せず無採番に
/// する。`SingleEnv`（split / multiline）は環境全体で 1 回だけ消費し（空ブロックには採番しない）、各行は
/// 無採番にする。`numbered == false` のときはいずれも採番しない。返り値は `(各行, 環境単位番号)` で、
/// 環境単位番号は `SingleEnv` で採番できたときのみ `Some`。
///
/// # Errors
///
/// 無採番の行への行ラベル付与（[`EvalError::LabelRequiresNumbering`]）、採番時の重複ラベル
/// （[`EvalError::DuplicateLabel`]）でエラーを返す。
pub(super) fn assign_numbering(
  grid: Vec<GridRow>,
  mode: &NumberingMode,
  numbered: bool,
  env_label: Option<&str>,
  view: &EnvironmentView,
  evaluator: &mut Evaluator,
) -> Result<(Vec<MathRow>, Option<String>), EvalError> {
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
        env_number =
          Some(evaluator.registry.increment_with_label(CounterName::Equation, env_label, view.span().into())?);
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
  return Ok((rows, env_number));
}
