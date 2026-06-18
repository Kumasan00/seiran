//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、`toml` クレートで
//! デシリアライズしてから `garde` で値検証を行い [`Style`] を返します。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。
//!
//! 既定値は各サブ struct の [`Default`] 実装が提供し、TOML 側は `#[serde(default)]` で
//! 部分指定をサポートします（未指定キーはデフォルト値で埋まる）。
//!
//! 各サブスタイル型はクレート直下のモジュール（[`caption`] / [`heading`] / [`figure`] 等）に置き、
//! [`Style`] がそれらをトップレベルのフィールドとして集約する。これらは `lowering` / `pdf_gen`
//! から参照される実働フィールドである。主要な型は本モジュールで再エクスポートする。

pub mod caption;
pub mod counter;
pub mod equation;
pub mod figure;
pub mod heading;
pub mod hyperref;
pub mod list;
pub mod math;
pub mod number_style;
pub mod page_numbering;
pub mod reference;
pub mod running;
pub mod table;
pub mod text;
pub mod title_page;
pub mod toc;

mod error;
mod style;

use std::{fs, path::Path};

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use tracing::info;

pub use crate::{
  caption::CaptionStyle,
  counter::{CounterName, CounterStyle, Counters},
  equation::{Alignment, EquationStyle, NumberSide},
  error::{ReadStyleError, ValidationError},
  figure::FigureStyle,
  heading::{HeadingStyle, HeadingStyles, default_for_level},
  hyperref::HyperrefStyle,
  list::ListStyle,
  math::MathScriptStyle,
  number_style::NumberStyle,
  page_numbering::PageNumbering,
  reference::ReferenceStyle,
  running::RunningContentStyle,
  style::Style,
  table::TableStyle,
  text::TextBlockStyle,
  title_page::TitlePageStyle,
  toc::TocStyle,
};

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はファイルを読み込まずに [`Style::default`] を返します。
/// パスが指定された場合はファイル内容を読み出し、[`parse_style`] へ委譲します。
///
/// # Errors
///
/// - ファイルが読めない場合は [`ReadStyleError::ReadFile`]
/// - TOML 解析に失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
// 設定ファイルは 1 回しか読まないため、Result サイズを最適化する価値が低い。
#[allow(clippy::result_large_err)]
pub fn read_style(path: Option<&Path>) -> Result<Style, ReadStyleError> {
  let Some(path) = path else {
    info!("スタイル設定ファイルが指定されていないため、デフォルト値を使用します");
    return Ok(Style::default());
  };
  let path_str = path.display().to_string();
  info!(style_path = %path_str, "スタイル設定ファイルの読み込みを開始します");

  let content = fs::read_to_string(path).map_err(|source| ReadStyleError::ReadFile {
    path: path_str.clone(),
    source,
  })?;

  let style = parse_style(&content, &path_str)?;

  info!(
    font_size_pt = style.font_size.to_pt(),
    line_height_factor = style.line_height_factor,
    "スタイル設定ファイルの読み込みが完了しました"
  );
  return Ok(style);
}

/// TOML 文字列を [`Style`] にパースし、値検証まで実行します（I/O なし）。
///
/// 未指定フィールドは `#[serde(default)]` 経由で [`Style::default`] の値が入ります。
///
/// # Errors
///
/// - TOML 解析に失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
#[allow(clippy::result_large_err)]
pub fn parse_style(content: &str, source_path: &str) -> Result<Style, ReadStyleError> {
  // `Style` は `#[serde(deny_unknown_fields)]` を持つため、未知のトップレベルキーは
  // この toml::from_str がそのまま span 付きで弾く。
  let style: Style = toml::from_str(content).map_err(|source| {
    let src = NamedSource::new(source_path, content.to_string());
    let span = source.span().map_or_else(
      || SourceSpan::new(0.into(), 0),
      |range| SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    ReadStyleError::ParseToml { src, span, source }
  })?;
  if let Err(errors) = validate_values(&style) {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }
  return Ok(style);
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証を本体（`#[garde(dive)]` フィールド）と `heading` の 2 系統で実行します。
/// カウンタの `resets` は固定 9 種の [`CounterName`] 配列として型付けされているため、
/// 不正名は TOML パース時点で拒否されます（追加のクロスフィールド検証は不要）。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(style: &Style) -> Result<(), Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();

  // Style 本体を検証する。パス文字列はそのまま TOML のキー階層と一致する。
  if let Err(report) = style.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }

  // HeadingStyles は #[garde(skip)] にしているため別途検証する
  for (level, heading) in style.heading.iter_with_level() {
    if let Err(report) = heading.validate() {
      errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
        path: format!("heading.{}.{path}", level.command_name()),
        message: error.to_string(),
      }));
    }
  }

  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
}
