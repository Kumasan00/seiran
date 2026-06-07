//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、`toml` クレートで
//! デシリアライズしてから `garde` で値検証を行い [`Style`] を返します。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。
//!
//! 既定値は各サブ struct の [`Default`] 実装が提供し、TOML 側は `#[serde(default)]` で
//! 部分指定をサポートします（未指定キーはデフォルト値で埋まる）。
//!
//! モジュールは [`core`] / [`extended`] / [`primitives`] の 3 階層に分かれている。
//! [`Style`] は `core: CoreStyle` と `extended: ExtendedStyle` の 2 層構造で、
//! `core` は `lowering` / `pdf_gen` から参照される実働フィールド、`extended` は
//! 未参照フィールド（脚注・目次・参考文献等）を保持する。

pub mod core;
mod error;
pub mod extended;
pub mod primitives;
mod style;

use std::{fs, path::Path};

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use serde::de::Error as _;
use tracing::info;
pub use types::Length;

pub use crate::{
  core::{
    CoreStyle,
    caption::CaptionStyle,
    counter::{CounterName, CounterStyle, Counters, NumberStyle},
    equation::{Alignment, EquationStyle, NumberSide},
    figure::FigureStyle,
    heading::{HeadingStyle, HeadingStyles, default_for_level},
    list::ListStyle,
    math::MathScriptStyle,
    table::TableStyle,
    text::TextBlockStyle,
  },
  error::{ReadStyleError, ValidationError},
  extended::{
    ExtendedStyle, footnote::FootnoteStyle, hyperref::HyperrefStyle, reference::ReferenceStyle, toc::TocStyle,
  },
  primitives::color::Color,
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
    font_size_pt = style.core.font_size.to_pt(),
    line_height_factor = style.core.line_height_factor,
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
  // 先にトップレベルキーの未知名検出を行う。
  // `Style` は `#[serde(flatten)]` で `core: CoreStyle` を展開しているため、
  // `deny_unknown_fields` は serde の制約で機能しない。代わりに toml::Value に一旦パースして
  // 既知キーの集合と照合する。
  reject_unknown_top_level_keys(content, source_path)?;

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

/// TOML のトップレベルキーが [`CoreStyle`] のフィールド名または `"extended"` に
/// 含まれるかをチェックする。未知のキーがあれば [`ReadStyleError::ParseToml`] を返す。
///
/// `#[serde(flatten)]` を使うと `deny_unknown_fields` が無効化されるため、
/// 別途このチェックを差し挟んで TOML 上の typo を捕捉する。
#[allow(clippy::result_large_err)]
fn reject_unknown_top_level_keys(content: &str, source_path: &str) -> Result<(), ReadStyleError> {
  const ALLOWED_KEYS: &[&str] = &[
    "font_size",
    "line_height_factor",
    "background_color",
    "heading",
    "text",
    "list",
    "math",
    "table",
    "figure",
    "equation",
    "counters",
    "extended",
  ];

  // 構文不正は本処理側のパースエラーで取得した方が span が正しいため、ここでは無視する
  let Ok(value) = toml::from_str::<toml::Table>(content) else {
    return Ok(());
  };
  for key in value.keys() {
    if !ALLOWED_KEYS.contains(&key.as_str()) {
      let src = NamedSource::new(source_path, content.to_string());
      let synthetic = toml::de::Error::custom(format!(
        "未知のトップレベルキー `{key}` が指定されています。許可されるキー: {ALLOWED_KEYS:?}"
      ));
      return Err(ReadStyleError::ParseToml {
        src,
        span: SourceSpan::new(0.into(), 0),
        source: synthetic,
      });
    }
  }
  return Ok(());
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証を `core` / `extended` / `heading` の 3 系統で実行します。
/// カウンタの `resets` は固定 9 種の [`CounterName`] 配列として型付けされているため、
/// 不正名は TOML パース時点で拒否されます（追加のクロスフィールド検証は不要）。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(style: &Style) -> Result<(), Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();

  // core / extended を個別に検証してパス文字列を TOML のキー階層と一致させる
  // （Style.validate() 経由だと `core.` プレフィックスが付いて TOML と乖離するため）
  if let Err(report) = style.core.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }
  if let Err(report) = style.extended.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: format!("extended.{path}"),
      message: error.to_string(),
    }));
  }

  // HeadingStyles は #[garde(skip)] にしているため別途検証する
  for (level, heading) in style.core.heading.iter_with_level() {
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
