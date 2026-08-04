//! 複数行数式環境の採番判定（任意引数の解釈と `MathRow` への割当）
//!
//! 実際の発番は行わず、採番対象とラベルだけを構造化する。

use miette::SourceSpan;

use super::{GridRow, is_blank_row};
use crate::{
  frontend::{
    evaluator::{
      EvalError,
      opt_args::{OptType, collect_environment_opt_args, find_bool, find_string},
    },
    span_ext::ToSourceSpan,
    syntax::ast::EnvironmentView,
  },
  model::MathRow,
};

/// 行末マーカー `\label{...}` の診断用 `SourceSpan` を `MathRow::label_span`（`crate::model::Span`）へ変換する
///
/// Document IR は診断ライブラリに依存しないため、境界で変換する。
fn to_model_span(span: SourceSpan) -> crate::model::Span {
  let start = u32::try_from(span.offset()).unwrap_or(u32::MAX);
  let end = u32::try_from(span.offset() + span.len()).unwrap_or(u32::MAX);
  return crate::model::Span::new(start, end);
}

/// 採番の粒度
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
      span: view.span().to_source_span(),
    });
  }
  // 無採番の環境は参照番号を持たないため、環境単位ラベルとの併用を禁じる（equation と同じ規則）
  if !numbered && env_label.is_some() {
    return Err(EvalError::LabelRequiresNumbering {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
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
    if let Some(span) = last.label_span {
      return Err(EvalError::RowLabelNotAtRowEnd { span });
    }
    grid.pop();
  }
  return Ok(());
}

/// グリッドを採番粒度（`mode`）に応じて [`MathRow`] 列へ変換する
///
/// 実際の番号は付けず、行と環境の採番対象フラグだけを返す。
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
        let numbered_row = numbered && row.notag_span.is_none();
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
          label_span: row.label_span.map(to_model_span),
        });
      })
      .collect::<Result<Vec<MathRow>, EvalError>>()?,
    NumberingMode::SingleEnv => {
      env_numbered = numbered && !grid.is_empty();
      grid
        .into_iter()
        .map(|row| {
          return MathRow {
            cells: row.cells,
            numbered: false,
            label: None,
            label_span: None,
          };
        })
        .collect()
    },
  };
  return Ok((rows, env_numbered));
}
