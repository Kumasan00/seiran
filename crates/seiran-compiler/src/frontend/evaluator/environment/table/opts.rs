//! `table` 環境の任意引数（`columns` / `widths` / `label` / `breakable`）解析

use crate::{
  document::{ColumnAlign, ColumnWidth},
  frontend::{
    evaluator::{
      EvalError,
      opt_args::{self, OptKey, collect_environment_opt_args},
    },
    syntax::view::EnvironmentView,
  },
  length::Length,
};

/// `table[columns=left center right]`（列の揃え）
const COLUMNS: OptKey<String> = opt_args::string("columns");
/// `table[widths=auto 5cm *]`（列幅）
const WIDTHS: OptKey<String> = opt_args::string("widths");
/// `table[label=...]`（`\ref` からの参照用）
const LABEL: OptKey<String> = opt_args::string("label");
/// `table[breakable=...]`（改ページによる分割を許可するか。既定 `true`）
const BREAKABLE: OptKey<bool> = opt_args::boolean("breakable");

/// `table` 環境の任意引数を集約した構造体
pub(super) struct TableOpts {
  /// `columns` オプションの生文字列（未指定なら `None`）
  pub(super) columns_spec: Option<String>,
  /// `widths` オプションの生文字列（未指定なら `None`）
  pub(super) widths_spec: Option<String>,
  /// `label` オプション（`\ref` からの参照用）
  pub(super) label: Option<String>,
  /// 改ページによる表の分割を許可するか（既定 `true`）
  pub(super) breakable: bool,
}

/// `table` の任意引数（`columns` / `widths` / `label` / `breakable`）を収集してスカラー化する
///
/// 既定では `breakable` は `true`（改ページによる分割を許可）。
pub(super) fn collect_table_opts(view: &EnvironmentView<'_>) -> Result<TableOpts, EvalError> {
  let opts = collect_environment_opt_args(
    view,
    &[
      COLUMNS.decl(),
      WIDTHS.decl(),
      LABEL.decl(),
      BREAKABLE.decl(),
    ],
  )?;

  return Ok(TableOpts {
    columns_spec: opts.get(COLUMNS),
    widths_spec: opts.get(WIDTHS),
    label: opts.get(LABEL),
    breakable: opts.get(BREAKABLE).unwrap_or(true),
  });
}

/// `columns=left center right` の値を [`ColumnAlign`] の列に変換する
pub(super) fn parse_columns_spec(spec: &str, view: &EnvironmentView<'_>) -> Result<Vec<ColumnAlign>, EvalError> {
  let invalid = || {
    return EvalError::InvalidOptArgValue {
      name: "table".to_string(),
      key: "columns".to_string(),
      expected: "left / center / right の空白区切り".to_string(),
      span: view.span().into(),
    };
  };
  let tokens: Vec<&str> = spec.split_whitespace().collect();
  if tokens.is_empty() {
    return Err(invalid());
  }
  return tokens.iter().map(|t| return t.parse::<ColumnAlign>().ok().ok_or_else(invalid)).collect();
}

/// `widths=auto auto 5cm 0.3 *` の値を [`ColumnWidth`] の列に変換する
pub(super) fn parse_widths_spec(spec: &str, view: &EnvironmentView<'_>) -> Result<Vec<ColumnWidth>, EvalError> {
  let invalid = || {
    return EvalError::InvalidOptArgValue {
      name: "table".to_string(),
      key: "widths".to_string(),
      expected: "auto / <num>pt / <num>mm / <num>cm / 0〜1 の比率 / * の空白区切り".to_string(),
      span: view.span().into(),
    };
  };
  let tokens: Vec<&str> = spec.split_whitespace().collect();
  if tokens.is_empty() {
    return Err(invalid());
  }
  return tokens.iter().map(|t| return parse_width_token(t).ok_or_else(invalid)).collect();
}

/// `widths=` の 1 トークンを [`ColumnWidth`] に変換する
///
/// 受理する形式: `auto` / `*` / 正の長さ（書式は [`Length`] の `FromStr` と同じ）/
/// 単位のない `0` より大きく `1` 以下の小数（本文幅に対する比率）。
/// 単位の有無で長さと比率が決まるので、両者の読みがぶつかることはない。
fn parse_width_token(token: &str) -> Option<ColumnWidth> {
  if token == "auto" {
    return Some(ColumnWidth::Auto);
  }
  if token == "*" {
    return Some(ColumnWidth::Flex);
  }
  if let Ok(length) = token.parse::<Length>() {
    return length.is_positive().then_some(ColumnWidth::Fixed(length));
  }
  let ratio: f32 = token.parse().ok()?;
  if !(ratio.is_finite() && ratio > 0.0 && ratio <= 1.0) {
    return None;
  }
  return Some(ColumnWidth::Ratio(ratio));
}
