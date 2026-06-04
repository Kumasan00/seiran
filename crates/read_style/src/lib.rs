//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、figment による
//! デフォルト値マージと `garde` による値検証を行って [`Style`] を返します。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。

mod counter;
mod equation;
mod error;
mod figure;
mod font_kind;
mod footnote;
mod heading;
mod hyperref;
mod list;
mod math_layout;
mod paragraph;
mod reference;
mod style;
mod table;
mod toc;

use std::{fs, path::Path};

use figment2::{
  Figment,
  providers::{Format, Serialized, Toml},
};
use garde::Validate;
use miette::{NamedSource, SourceSpan};
use tracing::info;

pub use crate::{
  counter::{CounterStyle, NumberFormat},
  equation::{Alignment, EquationStyle, NumberSide},
  error::{ParseTomlError, ReadStyleError, ValidationError},
  figure::{CaptionPosition, FigureStyle},
  font_kind::FontKindConfig,
  footnote::FootnoteStyle,
  heading::HeadingStyle,
  hyperref::HyperrefStyle,
  list::ListStyle,
  math_layout::MathLayoutStyle,
  paragraph::ParagraphStyle,
  reference::ReferenceStyle,
  style::Style,
  table::TableStyle,
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
/// - TOML 解析またはデフォルト値とのマージに失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
// `ReadStyleError::ParseToml` は内側に `NamedSource<String>` と `figment2::Error` を含むため
// Err バリアントが大きくなるが、`read_style` は設定ファイルごとに 1 回しか呼ばれないため
// Result サイズは性能上の問題にならない。
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
/// figment で [`Style::default`] にマージし、続けて [`validate_values`] を呼び出します。
/// `source_path` はエラー報告に使う表示用パスです。
///
/// # Errors
///
/// - TOML 解析またはデフォルト値とのマージに失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
#[allow(clippy::result_large_err)]
fn parse_style(content: &str, source_path: &str) -> Result<Style, ReadStyleError> {
  let style: Style = Figment::from(Serialized::defaults(Style::default()))
    .merge(Toml::string(content))
    .extract()
    .map_err(|source| {
      let src = NamedSource::new(source_path, content.to_string());
      let errors = source
        .into_iter()
        .map(|error| ParseTomlError {
          key_path: format_key_path(&error.path),
          src: src.clone(),
          span: locate_figment_span(content, &error.path),
          source: error,
        })
        .collect();
      ReadStyleError::ParseToml { errors }
    })?;
  if let Err(errors) = validate_values(&style) {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }
  return Ok(style);
}

/// figment のキーパス配列を表示用の文字列に整形します。
///
/// トップレベル（パスが空）の場合は `"(root)"`、それ以外はドット区切りで結合します。
fn format_key_path(path: &[String]) -> String {
  if path.is_empty() {
    return "(root)".to_string();
  }
  return path.join(".");
}

/// figment のキーパスから TOML ソース内の該当箇所を推定し [`SourceSpan`] を返します。
///
/// 完全な TOML パーサではなく、行先頭にキーが出現する一般的な書き方
/// （`font_size = 12.0`、`[chapter]` セクション内の `font_size = ...` 等）を
/// 対象にした best-effort 探索です。見つからない場合は `0..0` を返し、
/// `#[label]` は表示されません（エラーメッセージ自体には影響しません）。
fn locate_figment_span(content: &str, path: &[String]) -> SourceSpan {
  let Some(leaf) = path.last() else {
    return SourceSpan::new(0.into(), 0);
  };
  let section_path = &path[..path.len() - 1];

  // セクション内を探す場合は、まずセクションヘッダの後ろまで進める。
  let start = if section_path.is_empty() {
    0
  } else {
    let header = format!("[{}]", section_path.join("."));
    match content.find(&header) {
      Some(pos) => pos + header.len(),
      None => return SourceSpan::new(0.into(), 0),
    }
  };

  // セクション内（または先頭から）で、行頭の leaf キーを `^<key>\s*=` のパターンで探す。
  // `split_inclusive('\n')` で区切りを含めた長さを維持し、LF / CRLF どちらでも cursor がずれないようにする。
  let mut cursor = start;
  for raw_line in content[start..].split_inclusive('\n') {
    let line = raw_line.trim_end_matches('\n').trim_end_matches('\r');
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    // 次のセクションに入ったら打ち切る（同名キーの誤検出を防ぐ）
    if !section_path.is_empty() && trimmed.starts_with('[') {
      break;
    }
    if let Some(rest) = trimmed.strip_prefix(leaf.as_str()) {
      let after = rest.trim_start_matches([' ', '\t']);
      if after.starts_with('=') {
        return SourceSpan::new((cursor + leading).into(), leaf.len());
      }
    }
    cursor += raw_line.len();
  }
  return SourceSpan::new(0.into(), 0);
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証を集約します。
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

  use super::{CaptionPosition, ReadStyleError, Style, ValidationError, parse_style, read_style, validate_values};

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
    assert!(style.text_color.is_none());
    assert_eq!(style.part.format, default.part.format);
    assert_eq!(style.figure.caption_position, CaptionPosition::Bottom);
    assert_eq!(style.table.caption_position, CaptionPosition::Top);
    assert!(style.counters.contains_key("figure"));
  }

  #[test]
  fn parse_style_overrides_figure_caption_position() {
    // Arrange: figure テーブルだけ上書き
    let toml = "[figure]\ncaption_position = \"top\"\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    assert_eq!(style.figure.caption_position, CaptionPosition::Top);
    // 他のフィールドはデフォルト維持
    let default = Style::default();
    assert!((style.figure.caption_font_size - default.figure.caption_font_size).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_style_overrides_counter_display_name() {
    // Arrange: counters テーブルに日本語 display_name を上書き
    let toml = "[counters.figure]\ndisplay_name = \"図\"\nformat = \"prefixed\"\nparent = \"chapter\"\nresets = []\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    let figure = style.counters.get("figure").expect("figure counter");
    assert_eq!(figure.display_name, "図");
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
    let toml = "background_color = [204, 179, 153]\n";

    // Act
    let style = parse_style(toml, dummy_source()).unwrap();

    // Assert
    let [r, g, b] = style.background_color.expect("background_color should be Some");
    assert_eq!(r, 204);
    assert_eq!(g, 179);
    assert_eq!(b, 153);
  }

  #[test]
  fn parse_style_fails_on_unknown_top_level_key() {
    // Arrange: `font_size` のタイポ
    let toml = "font_sze = 15.0\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert: 未知キーは ParseToml として弾かれる
    assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
  }

  #[test]
  fn parse_style_fails_on_unknown_nested_key() {
    // Arrange: `font_size` のタイポ（chapter テーブル内）
    let toml = "[chapter]\nfont_sze = 30.0\n";

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
  fn parse_toml_error_carries_named_source_and_key_path() {
    // Arrange: font_size に型不一致を起こす
    let toml = "font_size = \"not a number\"\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    let Err(ReadStyleError::ParseToml { errors }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    let error = errors.first().expect("at least one error");
    assert_eq!(error.key_path, "font_size");
    assert_eq!(error.src.name(), dummy_source());
    // span が "font_size" の位置を指していること
    assert_eq!(error.span.offset(), 0);
    assert_eq!(error.span.len(), "font_size".len());
  }

  #[test]
  fn parse_toml_error_locates_nested_key_in_section() {
    // Arrange: chapter.font_size に型不一致
    let toml = "[chapter]\nfont_size = \"oops\"\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    let Err(ReadStyleError::ParseToml { errors }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    let error = errors.first().expect("at least one error");
    assert_eq!(error.key_path, "chapter.font_size");
    // span は section 内 "font_size" の絶対オフセットを指す
    let expected_offset = toml.find("font_size = \"oops\"").unwrap();
    assert_eq!(error.span.offset(), expected_offset);
    assert_eq!(error.span.len(), "font_size".len());
  }

  #[test]
  fn parse_toml_error_locates_nested_key_in_section_with_crlf() {
    // Arrange: 行末を CRLF にしても section 内のキー位置が正しく出ること
    let toml = "[chapter]\r\nfont_size = \"oops\"\r\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    let Err(ReadStyleError::ParseToml { errors }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    let error = errors.first().expect("at least one error");
    assert_eq!(error.key_path, "chapter.font_size");
    let expected_offset = toml.find("font_size = \"oops\"").unwrap();
    assert_eq!(error.span.offset(), expected_offset);
    assert_eq!(error.span.len(), "font_size".len());
  }

  #[test]
  fn parse_style_fails_on_background_color_overflow() {
    // Arrange: R 成分が 256（u8 の範囲外）
    let toml = "background_color = [256, 0, 0]\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert: u8 にデシリアライズできないため ParseToml として弾かれる
    assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
  }

  #[test]
  fn parse_style_fails_on_background_color_negative() {
    // Arrange: B 成分が -1（u8 は符号なし）
    let toml = "background_color = [0, 0, -1]\n";

    // Act
    let result = parse_style(toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
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
