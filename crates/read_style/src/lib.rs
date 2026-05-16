//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、figment による
//! デフォルト値マージと `garde` による値検証を行って [`Style`] を返します。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。

use std::{fs, path::Path};

use figment2::{
  Figment,
  providers::{Format, Serialized, Toml},
};
use garde::Validate;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

/// スタイル設定ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ReadStyleError {
  /// スタイル設定ファイルの読み込み失敗（I/O エラー）
  #[error("スタイル設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(style::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// スタイル設定の TOML 解析または既定値とのマージに失敗した場合
  #[error("スタイル設定ファイルの解析に失敗しました: {path}")]
  #[diagnostic(code(style::parse_toml), help("TOML の構文とフィールドの型を確認してください。"))]
  ParseToml {
    /// ファイルパス
    path: String,
    /// 元のエラー
    #[source]
    source: Box<figment2::Error>,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("スタイル設定のバリデーションに失敗しました。")]
  #[diagnostic(code(style::multiple_validation_errors))]
  MultipleValidationErrors {
    /// 検証で検出されたすべてのエラー
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// スタイル設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// garde が検出したスタイル設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(style::validation::field), help("style.toml の該当フィールドの値を確認してください。"))]
  Field {
    /// 不正なフィールドのパス（例: `font_size`, `part.font_size`）
    path: String,
    /// 不正の内容
    message: String,
  },
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Style {
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 背景色 RGB（0.0-1.0、オプション）。未指定時は背景色なし。
  #[garde(custom(validate_background_color))]
  pub background_color: Option<[f32; 3]>,
  #[garde(dive)]
  pub part: HeadingStyle,
  #[garde(dive)]
  pub chapter: HeadingStyle,
  #[garde(dive)]
  pub section: HeadingStyle,
  #[garde(dive)]
  pub sub_section: HeadingStyle,
  #[garde(dive)]
  pub paragraph: HeadingStyle,
  #[garde(dive)]
  pub sub_paragraph: HeadingStyle,
  #[garde(dive)]
  pub reference: ReferenceStyle,
  // TODO(figure-equation-prep): figure / equation / table 用 *Style 構造体の追加予定地。
  // 実装本体タスクで [counters] テーブルおよび FigureStyle / EquationStyle /
  // TableStyle を追加し、`parser::evaluator::counter::CounterRegistry::from_style`
  // と組み合わせて lowering までカスタマイズできるようにする。
}

impl Default for Style {
  fn default() -> Self {
    // 軸補 i18n: デフォルトは英語。日本語化したい場合は style.toml で上書きする。
    // プレースホルダは `{number}` `{title}` のみ:
    //   - `{number}` は HeadingNumber::dotted() の値（Part: "1"、Chapter: "1"、Section: "1.2"、…）
    //   - `{title}` は見出しタイトルのプレーンテキスト
    let part = "Part {number}: {title}".to_string();
    let chapter = "Chapter {number}: {title}".to_string();
    let section = "{number} {title}".to_string();
    let sub_section = "{number} {title}".to_string();
    let paragraph = "{number} {title}".to_string();
    let sub_paragraph = "{number} {title}".to_string();
    return Self {
      font_size: 12.0,
      line_height_factor: 1.2,
      background_color: None,
      part: HeadingStyle::new(part, 40.0, 20.0, true, true),
      chapter: HeadingStyle::new(chapter, 25.0, 15.0, true, false),
      section: HeadingStyle::new(section, 20.0, 10.0, false, false),
      sub_section: HeadingStyle::new(sub_section, 16.0, 10.0, false, false),
      paragraph: HeadingStyle::new(paragraph, 14.0, 5.0, false, false),
      sub_paragraph: HeadingStyle::new(sub_paragraph, 12.0, 5.0, false, false),
      reference: ReferenceStyle::default(),
    };
  }
}

/// `background_color` の各成分が [0.0, 1.0] の範囲かを検証します。
///
/// `None` はそのまま通過させます。NaN や Infinity は範囲チェックで自動的に弾かれます。
/// 引数の型は `garde` のカスタムバリデーター API に従います。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_background_color(value: &Option<[f32; 3]>, _: &()) -> garde::Result {
  let Some([r, g, b]) = value else {
    return Ok(());
  };
  for (component, v) in [("R", *r), ("G", *g), ("B", *b)] {
    if !(0.0..=1.0).contains(&v) {
      return Err(garde::Error::new(format!(
        "background_color の {component} 成分は [0.0, 1.0] の範囲である必要があります: {v}"
      )));
    }
  }
  return Ok(());
}

/// 見出し要素のスタイル設定（フォントサイズと下余白）
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct HeadingStyle {
  pub format: String,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  pub page_break_before: bool,
  pub page_break_after: bool,
}

impl HeadingStyle {
  /// 新しい [`HeadingStyle`] を作成する
  #[must_use]
  const fn new(
    format: String,
    font_size: f32,
    bottom_margin: f32,
    page_break_before: bool,
    page_break_after: bool,
  ) -> Self {
    return Self {
      format,
      font_size,
      bottom_margin,
      page_break_before,
      page_break_after,
    };
  }
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct ReferenceStyle {
  pub format: String,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      format: "References".to_string(),
      font_size: 12.0,
      bottom_margin: 10.0,
    };
  }
}

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はファイルを読み込まずに [`Style::default`] を返します。
/// パスが指定された場合はファイル内容を読み出し、[`parse_style`] へ委譲します。
///
/// # Errors
///
/// - ファイルが読めない場合は [`ReadStyleError::ReadFile`]
/// - TOML 解析またはデフォルト値とのマージに失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
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
/// figment で [`Style::default`] にマージし、続けて [`validate_values`] を呼び出します。
/// `source_path` はエラー報告に使う表示用パスです。
///
/// # Errors
///
/// - TOML 解析またはデフォルト値とのマージに失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
fn parse_style(content: &str, source_path: &str) -> Result<Style, ReadStyleError> {
  let style: Style = Figment::from(Serialized::defaults(Style::default()))
    .merge(Toml::string(content))
    .extract()
    .map_err(|source| ReadStyleError::ParseToml {
      path: source_path.to_string(),
      source: Box::new(source),
    })?;
  if let Err(errors) = validate_values(&style) {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }
  return Ok(style);
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証および `background_color` のカスタム検証を集約します。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(style: &Style) -> Result<(), Vec<ValidationError>> {
  let Err(report) = style.validate() else {
    return Ok(());
  };
  let errors = report
    .iter()
    .map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    })
    .collect();
  return Err(errors);
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{ReadStyleError, Style, ValidationError, parse_style, read_style, validate_values};

  /// `parse_style` のエラー報告に使うダミーパス
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
    assert_eq!(style.part.format, default.part.format);
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
  fn parse_style_returns_defaults_for_empty_content() {
    // Arrange / Act
    let style = parse_style("", dummy_source()).unwrap();

    // Assert
    let default = Style::default();
    assert!((style.font_size - default.font_size).abs() < f32::EPSILON);
    assert_eq!(style.part.format, default.part.format);
    assert_eq!(style.chapter.format, default.chapter.format);
  }

  #[test]
  fn parse_style_overrides_only_specified_fields() {
    // Arrange: font_size のみ上書き、他はデフォルト維持
    let toml = "font_size = 15.0\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    assert!((style.font_size - 15.0).abs() < f32::EPSILON);
    let default = Style::default();
    assert!((style.line_height_factor - default.line_height_factor).abs() < f32::EPSILON);
    assert_eq!(style.part.format, default.part.format);
  }

  #[test]
  fn parse_style_overrides_nested_heading_style() {
    // Arrange: chapter テーブルだけ上書きしても part 等はデフォルト維持
    let toml = "[chapter]\nformat = \"第{number}章 {title}\"\nfont_size = 30.0\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    assert_eq!(style.chapter.format, "第{number}章 {title}");
    assert!((style.chapter.font_size - 30.0).abs() < f32::EPSILON);
    let default = Style::default();
    assert_eq!(style.part.format, default.part.format);
    assert_eq!(style.section.format, default.section.format);
  }

  #[test]
  fn parse_style_accepts_valid_background_color() {
    // Arrange
    let toml = "background_color = [0.8, 0.7, 0.6]\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    let [r, g, b] = style.background_color.expect("background_color should be Some");
    assert!((r - 0.8).abs() < f32::EPSILON);
    assert!((g - 0.7).abs() < f32::EPSILON);
    assert!((b - 0.6).abs() < f32::EPSILON);
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
  fn parse_style_fails_on_zero_font_size() {
    // Arrange: range(min = f32::MIN_POSITIVE) 違反
    let toml = "font_size = 0.0\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "font_size"
    )));
  }

  #[test]
  fn parse_style_fails_on_negative_line_height_factor() {
    // Arrange
    let toml = "line_height_factor = -1.0\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "line_height_factor"
    )));
  }

  #[test]
  fn parse_style_fails_on_background_color_above_one() {
    // Arrange: R 成分が 1.1（> 1.0）
    let toml = "background_color = [1.1, 0.5, 0.5]\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message }
        if path == "background_color" && message.contains('R')
    )));
  }

  #[test]
  fn parse_style_fails_on_background_color_negative() {
    // Arrange: B 成分が -0.1（< 0.0）
    let toml = "background_color = [0.5, 0.5, -0.1]\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message }
        if path == "background_color" && message.contains('B')
    )));
  }

  #[test]
  fn parse_style_fails_on_background_color_nan() {
    // Arrange: TOML 1.0 は `nan` を許容するが、validator が範囲外として弾く
    let toml = "background_color = [nan, 0.5, 0.5]\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "background_color"
    )));
  }

  #[test]
  fn parse_style_fails_on_negative_heading_bottom_margin() {
    // Arrange: chapter.bottom_margin が負値
    let toml = "[chapter]\nbottom_margin = -5.0\n";

    // Act
    let errors = expect_validation_errors(parse_style(toml, dummy_source()));

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "chapter.bottom_margin"
    )));
  }

  #[test]
  fn parse_style_collects_multiple_validation_errors() {
    // Arrange: font_size と chapter.font_size の両方を不正値に
    let toml = "font_size = 0.0\n\n[chapter]\nfont_size = -1.0\n";

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
    assert!(paths.contains(&"chapter.font_size"));
  }

  #[test]
  fn validate_values_accepts_default_style() {
    // Arrange / Act / Assert: デフォルト値はバリデーションを通過する
    assert!(validate_values(&Style::default()).is_ok());
  }

  /// `parse_style` の戻り値から `MultipleValidationErrors` を取り出すヘルパー。
  fn expect_validation_errors(result: Result<Style, ReadStyleError>) -> Vec<ValidationError> {
    match result {
      Err(ReadStyleError::MultipleValidationErrors { errors }) => return errors,
      other => panic!("expected MultipleValidationErrors, got {other:?}"),
    }
  }
}
