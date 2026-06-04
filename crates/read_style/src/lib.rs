//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、`toml` クレートで
//! デシリアライズしてから `garde` で値検証を行い [`Style`] を返します。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。
//!
//! 既定値は各サブ struct の [`Default`] 実装が提供し、TOML 側は `#[serde(default)]` で
//! 部分指定をサポートします（未指定キーはデフォルト値で埋まる）。

pub mod block;
pub mod common;
pub mod cross_ref;
mod error;
mod extended;
pub mod float;
mod style;

use std::{fs, path::Path};

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use tracing::info;

pub use crate::{
  block::{
    heading::{HeadingStyle, default_for_level, default_per_level},
    list::ListStyle,
    math::MathScriptStyle,
    text::TextBlockStyle,
  },
  common::{
    caption::{CaptionPosition, CaptionStyle},
    color::Color,
    counter::{CounterEntry, CounterStyle, NumberFormat, default_counters},
    length::Length,
    per_level::PerLevel,
  },
  cross_ref::{hyperref::HyperrefStyle, reference::ReferenceStyle, toc::TocStyle},
  error::{ReadStyleError, ValidationError},
  extended::ExtendedStyle,
  float::{
    equation::{Alignment, EquationStyle, NumberSide},
    figure::FigureStyle,
    footnote::FootnoteStyle,
    table::TableStyle,
  },
  style::Style,
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
/// `garde` のフィールド検証に加え、カウンタの parent / resets が `counters` に
/// 実在することを確認するクロスフィールド検証を行います。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(style: &Style) -> Result<(), Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();

  // garde 派生による単一フィールド検証
  if let Err(report) = style.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }

  // PerLevel<HeadingStyle> は #[garde(skip)] にしているため別途検証する
  for (level, heading) in style.heading.iter_with_level() {
    if let Err(report) = heading.validate() {
      errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
        path: format!("heading.{}.{path}", level.command_name()),
        message: error.to_string(),
      }));
    }
  }

  // クロスフィールド: counters の parent / resets / alias_of が counters に実在するか
  for (name, entry) in &style.counters {
    match entry {
      CounterEntry::Alias(alias) => {
        if !style.counters.contains_key(&alias.alias_of) {
          errors.push(ValidationError::Field {
            path: format!("counters.{name}.alias_of"),
            message: format!("別名のソース '{}' が counters に存在しません", alias.alias_of),
          });
        }
      },
      CounterEntry::Counter(def) => {
        if let Some(parent) = &def.parent
          && !style.counters.contains_key(parent)
        {
          errors.push(ValidationError::Field {
            path: format!("counters.{name}.parent"),
            message: format!("親カウンタ '{parent}' が counters に存在しません"),
          });
        }
        for reset in &def.resets {
          if !style.counters.contains_key(reset) {
            errors.push(ValidationError::Field {
              path: format!("counters.{name}.resets"),
              message: format!("リセット対象 '{reset}' が counters に存在しません"),
            });
          }
        }
      },
    }
  }

  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
}
