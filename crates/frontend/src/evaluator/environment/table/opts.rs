//! `table` 環境の任意引数（`columns` / `widths` / `label` / `breakable`）解析

use model::{ColumnAlign, ColumnWidth, Length};

use crate::{
  evaluator::{
    EvalError,
    opt_args::{OptType, OptValue, collect_environment_opt_args},
  },
  span_ext::ToSourceSpan,
  syntax::ast::EnvironmentView,
};

/// `table` 環境の任意引数を集約した構造体
pub(super) struct TableOpts {
  pub(super) columns_spec: Option<String>,
  pub(super) widths_spec: Option<String>,
  pub(super) label: Option<String>,
  pub(super) breakable: bool,
}

/// `table` の任意引数（`columns` / `widths` / `label` / `breakable`）を収集してスカラー化する
///
/// 既定では `breakable` は `true`（改ページによる分割を許可）。
pub(super) fn collect_table_opts(view: &EnvironmentView) -> Result<TableOpts, EvalError> {
  let opt_args = collect_environment_opt_args(
    view,
    &[
      ("columns", OptType::String),
      ("widths", OptType::String),
      ("label", OptType::String),
      ("breakable", OptType::Bool),
    ],
  )?;

  let mut columns_spec: Option<String> = None;
  let mut widths_spec: Option<String> = None;
  let mut label: Option<String> = None;
  let mut breakable = true;
  for (key, value) in opt_args {
    match (key.as_str(), value) {
      ("columns", OptValue::String(s)) => columns_spec = Some(s),
      ("widths", OptValue::String(s)) => widths_spec = Some(s),
      ("label", OptValue::String(s)) => label = Some(s),
      ("breakable", OptValue::Bool(b)) => breakable = b,
      _ => {},
    }
  }

  return Ok(TableOpts {
    columns_spec,
    widths_spec,
    label,
    breakable,
  });
}

/// `columns="left center right"` の値を [`ColumnAlign`] の列に変換する
pub(super) fn parse_columns_spec(spec: &str, view: &EnvironmentView) -> Result<Vec<ColumnAlign>, EvalError> {
  let invalid = || EvalError::InvalidOptArgValue {
    name: "table".to_string(),
    key: "columns".to_string(),
    expected: "left / center / right の空白区切り".to_string(),
    span: view.span().to_source_span(),
  };
  let tokens: Vec<&str> = spec.split_whitespace().collect();
  if tokens.is_empty() {
    return Err(invalid());
  }
  return tokens.iter().map(|t| ColumnAlign::from_keyword(t).ok_or_else(invalid)).collect();
}

/// `widths="auto auto 5cm 0.3 *"` の値を [`ColumnWidth`] の列に変換する
pub(super) fn parse_widths_spec(spec: &str, view: &EnvironmentView) -> Result<Vec<ColumnWidth>, EvalError> {
  let invalid = || EvalError::InvalidOptArgValue {
    name: "table".to_string(),
    key: "widths".to_string(),
    expected: "auto / <num>mm / <num>cm / 0〜1 の比率 / * の空白区切り".to_string(),
    span: view.span().to_source_span(),
  };
  let tokens: Vec<&str> = spec.split_whitespace().collect();
  if tokens.is_empty() {
    return Err(invalid());
  }
  return tokens.iter().map(|t| parse_width_token(t).ok_or_else(invalid)).collect();
}

/// `widths=` の 1 トークンを [`ColumnWidth`] に変換する
///
/// 受理する形式: `auto` / `*` / `<num>mm` / `<num>cm`（サフィックスは大小無視）/
/// `0` より大きく `1` 以下の小数（本文幅に対する比率）。
fn parse_width_token(token: &str) -> Option<ColumnWidth> {
  if token == "auto" {
    return Some(ColumnWidth::Auto);
  }
  if token == "*" {
    return Some(ColumnWidth::Flex);
  }
  let lower = token.to_ascii_lowercase();
  if let Some(stripped) = lower.strip_suffix("mm") {
    let value: f32 = stripped.parse().ok()?;
    if !(value.is_finite() && value > 0.0) {
      return None;
    }
    return Some(ColumnWidth::Fixed(Length::mm(value)));
  }
  if let Some(stripped) = lower.strip_suffix("cm") {
    let value: f32 = stripped.parse().ok()?;
    if !(value.is_finite() && value > 0.0) {
      return None;
    }
    return Some(ColumnWidth::Fixed(Length::cm(value)));
  }
  let ratio: f32 = lower.parse().ok()?;
  if !(ratio.is_finite() && ratio > 0.0 && ratio <= 1.0) {
    return None;
  }
  return Some(ColumnWidth::Ratio(ratio));
}
