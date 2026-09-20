//! 複数行数式環境の採番判定（任意引数の解釈と `HirMathRow` への割当）
//!
//! 実際の発番は行わず、採番対象とラベルだけを構造化する。

use crate::{
  document::HirMathRow,
  frontend::{
    evaluator::{
      EvalError, arity,
      environment::math::math_grid::{GridRow, is_blank_row},
      opt_args::{self, OptDecl, OptKey, collect_environment_opt_args},
    },
    syntax::view::EnvironmentView,
  },
};

/// 数式環境の `[label=...]`（環境単位ラベル）
const LABEL: OptKey<String> = opt_args::string("label");
/// 数式環境の `[numbered=...]`（既定 `true`）
const NUMBERED: OptKey<bool> = opt_args::boolean("numbered");
/// 環境単位ラベルを受理する環境（split / multiline）のスキーマ
const SINGLE_ENV_SCHEMA: &[OptDecl] = &[LABEL.decl(), NUMBERED.decl()];
/// 行ごと採番の環境（align / gather）のスキーマ — 環境単位ラベルは受理しない
const PER_ROW_SCHEMA: &[OptDecl] = &[NUMBERED.decl()];

/// 採番の粒度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberingMode {
  /// 各行を採番対象にする（`align` / `gather`）
  PerRow,
  /// 環境全体を採番対象にする（`split` / `multiline`）
  SingleEnv,
}

/// 数式環境の任意引数 `[numbered]` / `[label=...]` を解析・検証する
///
/// 環境ラベルは [`NumberingMode::SingleEnv`] の場合だけ受理する。
///
/// # Errors
///
/// 未知の任意引数キー・不正な値、位置引数の指定（[`EvalError::ExtraEnvironmentArgument`]）、無採番環境への
/// 環境単位ラベル付与（[`EvalError::LabelRequiresNumbering`]）でエラーを返す。
pub(super) fn parse_math_env_opts(
  view: &EnvironmentView<'_>,
  mode: NumberingMode,
) -> Result<(bool, Option<String>), EvalError> {
  // 環境単位ラベル `[label=...]` は環境全体に 1 番号を振る `SingleEnv`（split / multiline）でのみ受理する。
  // 行ごと採番（`PerRow` = align / gather）の行単位ラベルは行末マーカー `\label{...}` で指定する。
  let allow_env_label = matches!(mode, NumberingMode::SingleEnv);
  let schema = if allow_env_label {
    SINGLE_ENV_SCHEMA
  } else {
    PER_ROW_SCHEMA
  };
  let opts = collect_environment_opt_args(view, schema)?;
  let numbered = opts.get(NUMBERED).unwrap_or(true);
  let env_label = opts.get(LABEL);
  arity::no_environment_args(view)?;
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
    if let Some(label) = &last.label {
      return Err(EvalError::RowLabelNotAtRowEnd { span: label.span });
    }
    grid.pop();
  }
  return Ok(());
}

/// グリッドを採番粒度（`mode`）に応じて [`HirMathRow`] 列へ変換する
///
/// 実際の番号は付けず、行と環境の採番対象フラグだけを返す。
///
/// # Errors
///
/// 無採番の行への行ラベル付与時に [`EvalError::LabelRequiresNumbering`] を返す。
pub(super) fn assign_numbering(
  grid: Vec<GridRow>,
  mode: NumberingMode,
  numbered: bool,
  view: &EnvironmentView<'_>,
) -> Result<(Vec<HirMathRow>, bool), EvalError> {
  let mut env_numbered = false;
  let rows: Vec<HirMathRow> = match mode {
    NumberingMode::PerRow => grid
      .into_iter()
      .map(|row| -> Result<HirMathRow, EvalError> {
        let numbered_row = numbered && row.notag_span.is_none();
        if let Some(label) = &row.label
          && !numbered_row
        {
          return Err(EvalError::LabelRequiresNumbering {
            name: view.name().to_string(),
            span: label.span,
          });
        }
        let (label, label_site) = match row.label {
          Some(label) => (Some(label.name), Some(label.site)),
          None => (None, None),
        };
        return Ok(HirMathRow {
          id: row.id,
          cells: row.cells,
          numbered: numbered_row,
          label,
          label_site,
        });
      })
      .collect::<Result<Vec<HirMathRow>, EvalError>>()?,
    NumberingMode::SingleEnv => {
      env_numbered = numbered && !grid.is_empty();
      grid
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
        .collect()
    },
  };
  return Ok((rows, env_numbered));
}
