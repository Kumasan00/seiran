//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、`toml` クレートで
//! デシリアライズしてから `garde` で値検証を行い [`Style`] を返します。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。
//!
//! 既定値は各サブ struct の [`Default`] 実装が提供し、TOML 側は `#[serde(default)]` で
//! 部分指定をサポートします（未指定キーはデフォルト値で埋まる）。

pub mod block;
pub mod color;
pub mod cross_ref;
mod error;
pub mod float;
pub mod per_level;
mod style;

use std::{fs, path::Path};

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use tracing::info;

pub use crate::{
  block::{
    heading::{HeadingStyle, default_for_level, default_per_level},
    list::ListStyle,
    math::MathStyle,
    text::TextBlockStyle,
  },
  color::Color,
  cross_ref::{
    counter::{CounterStyle, NumberFormat, default_counters},
    hyperref::HyperrefStyle,
    reference::ReferenceStyle,
    toc::TocStyle,
  },
  error::{ReadStyleError, ValidationError},
  float::{
    equation::{Alignment, EquationStyle, NumberSide},
    figure::{CaptionPosition, FigureStyle},
    footnote::FootnoteStyle,
    table::TableStyle,
  },
  per_level::PerLevel,
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
    font_size = style.font_size,
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
  for (name, counter) in &style.counters {
    if let Some(parent) = &counter.parent
      && !style.counters.contains_key(parent)
    {
      errors.push(ValidationError::Field {
        path: format!("counters.{name}.parent"),
        message: format!("親カウンタ '{parent}' が counters に存在しません"),
      });
    }
    for reset in &counter.resets {
      if !style.counters.contains_key(reset) {
        errors.push(ValidationError::Field {
          path: format!("counters.{name}.resets"),
          message: format!("リセット対象 '{reset}' が counters に存在しません"),
        });
      }
    }
    if let Some(alias) = &counter.alias_of
      && !style.counters.contains_key(alias)
    {
      errors.push(ValidationError::Field {
        path: format!("counters.{name}.alias_of"),
        message: format!("別名のソース '{alias}' が counters に存在しません"),
      });
    }
  }

  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use types::HeadingLevel;

  use super::{ReadStyleError, Style, ValidationError, parse_style, read_style, validate_values};

  fn dummy_source() -> &'static str { return "test.toml"; }

  #[test]
  fn read_style_returns_default_when_path_is_none() {
    // Arrange / Act
    let style = read_style(None).unwrap();

    // Assert
    let default = Style::default();
    assert!((style.font_size - default.font_size).abs() < f32::EPSILON);
    assert!((style.line_height_factor - default.line_height_factor).abs() < f32::EPSILON);
    assert!(style.background_color.is_none());
    assert!(style.text_color.is_none());
    assert_eq!(style.heading(HeadingLevel::Part).format, default.heading(HeadingLevel::Part).format);
  }

  #[test]
  fn parse_style_overrides_heading_section_format() {
    // Arrange: [heading.section] テーブルを上書き
    let toml = "[heading.section]\nformat = \"§ {number} {title}\"\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
    // 他のレベルはデフォルトを維持
    let default = Style::default();
    assert_eq!(style.heading(HeadingLevel::Chapter).format, default.heading(HeadingLevel::Chapter).format);
  }

  #[test]
  fn parse_style_overrides_only_specified_fields() {
    // Arrange: font_size のみ上書き
    let toml = "font_size = 15.0\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    assert!((style.font_size - 15.0).abs() < f32::EPSILON);
    let default = Style::default();
    assert!((style.line_height_factor - default.line_height_factor).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_style_accepts_color_array() {
    // Arrange
    let toml = "background_color = [204, 179, 153]\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    let color = style.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [204, 179, 153]);
  }

  #[test]
  fn parse_style_accepts_color_hex_string() {
    // Arrange
    let toml = "background_color = \"#cc9966\"\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    let color = style.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [0xcc, 0x99, 0x66]);
  }

  #[test]
  fn parse_style_fails_on_unknown_top_level_key() {
    // Arrange: typo
    let toml = "font_sze = 15.0\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
  }

  #[test]
  fn parse_style_fails_on_unknown_nested_key() {
    // Arrange: typo inside [heading.chapter]
    let toml = "[heading.chapter]\nfont_sze = 30.0\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
  }

  #[test]
  fn parse_style_fails_on_invalid_toml_syntax() {
    // Arrange
    let toml = "font_size = \nthis is not valid toml";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
  }

  #[test]
  fn parse_style_collects_multiple_validation_errors() {
    // Arrange: font_size と heading.chapter.font_size の両方を不正値に
    let toml = "font_size = 0.0\n\n[heading.chapter]\nfont_size = -1.0\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    let paths: Vec<&str> = errors
      .iter()
      .map(|error| match error {
        ValidationError::Field { path, .. } => path.as_str(),
      })
      .collect();
    assert!(paths.contains(&"font_size"));
    assert!(paths.contains(&"heading.chapter.font_size"));
  }

  #[test]
  fn validate_values_rejects_unknown_counter_parent() {
    // Arrange: parent に存在しないカウンタ名を指定
    let toml = "
[counters.invalid]
display_name = \"Invalid\"
parent = \"nonexistent\"
format = \"plain\"
resets = []
";
    let style: Style = toml::from_str(toml).expect("toml itself should parse");

    // Act
    let errors = validate_values(&style).expect_err("cross-field validation should fail");

    // Assert
    assert!(errors.iter().any(|e| matches!(e, ValidationError::Field { path, .. } if path.contains("parent"))));
  }

  #[test]
  fn validate_values_rejects_unknown_reset_target() {
    // Arrange
    let toml = "
[counters.custom]
display_name = \"Custom\"
parent = \"chapter\"
format = \"prefixed\"
resets = [\"nonexistent\"]
";
    let style: Style = toml::from_str(toml).expect("toml itself should parse");

    // Act
    let errors = validate_values(&style).expect_err("cross-field validation should fail");

    // Assert
    assert!(errors.iter().any(|e| matches!(e, ValidationError::Field { path, .. } if path.contains("resets"))));
  }

  #[test]
  fn read_style_fails_on_nonexistent_path() {
    // Arrange
    let path = PathBuf::from("/nonexistent/style.toml");

    // Act
    let result = read_style(Some(path.as_path()));

    // Assert
    assert!(matches!(result, Err(ReadStyleError::ReadFile { .. })));
  }

  #[test]
  fn validate_values_accepts_default_style() {
    assert!(validate_values(&Style::default()).is_ok());
  }

  fn expect_validation_errors(result: Result<Style, ReadStyleError>) -> Vec<ValidationError> {
    match result {
      Err(ReadStyleError::MultipleValidationErrors { errors }) => return errors,
      other => panic!("expected MultipleValidationErrors, got {other:?}"),
    }
  }
}
