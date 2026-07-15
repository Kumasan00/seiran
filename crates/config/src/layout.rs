//! `config`（用紙・余白）× `style`（`[columns]`）の横断バリデーション
//!
//! 段幅 = `(本文幅 − (段数 − 1) × 段間) / 段数` は [`Config`] と [`Style`] の双方の値がないと
//! 判定できないため、`read_config` / `read_style` それぞれの単体検証では検証できない
//! （旧構成ではこの理由で `seiran` のビルドステージに置かれていた）。両者が揃った時点で
//! [`validate_layout`] を呼ぶことで、config クレートがこの横断制約を一括で担う。

use miette::Diagnostic;
use model::column_width;
use thiserror::Error;

use crate::{Config, Style};

/// config × style 横断バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum LayoutValidationError {
  /// 段組み設定により 1 段あたりの幅が 0 以下になった場合
  ///
  /// 段幅 = `(本文幅 − (段数 − 1) × 段間) / 段数`。段間が本文幅に対して大きすぎると非正になる。
  #[error(
    "段組みの 1 段あたりの幅が 0 以下になりました（本文幅 {text_width:.1}pt / 段数 {num_columns} / 段間 {column_gap:.1}pt）。"
  )]
  #[diagnostic(
    code(config::validation::invalid_columns),
    help(
      "style.toml の [columns].gap を小さくするか、count を減らしてください。または config.toml の用紙幅を広げる・左右余白を狭めて本文幅を確保してください。"
    )
  )]
  InvalidColumnWidth {
    /// 本文幅（pt）
    text_width: f32,
    /// 段数
    num_columns: usize,
    /// 段間（pt）
    column_gap: f32,
  },
}

/// [`Config`]（用紙・余白）と [`Style`]（`[columns]`）の横断制約を検証します。
///
/// 本文幅（`pdf.width - margin.left - margin.right`）を `style.columns` の段数・段間で割った
/// 1 段あたりの幅が非正の場合にエラーを返します。
///
/// # Errors
///
/// 1 段あたりの幅が 0 以下の場合 [`LayoutValidationError::InvalidColumnWidth`] を返します。
pub fn validate_layout(config: &Config, style: &Style) -> Result<(), LayoutValidationError> {
  let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
  let num_columns = style.columns.count as usize;
  let column_gap = style.columns.gap;

  if !column_width(text_width, num_columns, column_gap).is_positive() {
    return Err(LayoutValidationError::InvalidColumnWidth {
      text_width: text_width.to_pt(),
      num_columns,
      column_gap: column_gap.to_pt(),
    });
  }
  return Ok(());
}
