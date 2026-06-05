//! TOML パース系の統合テスト。
//!
//! 構文エラー / 未知キー / 各種型（`Length` / `Color`）の受理/拒否を網羅する。
//! 検証エラーは `validate.rs` 側で扱う。

use std::path::PathBuf;

use read_style::{ReadStyleError, Style, parse_style, read_style};
use types::HeadingLevel;

fn dummy_source() -> &'static str { return "test.toml"; }

#[test]
fn read_style_returns_default_when_path_is_none() {
  // Arrange / Act
  let style = read_style(None).unwrap();

  // Assert
  let default = Style::default();
  assert!((style.core.font_size.to_pt() - default.core.font_size.to_pt()).abs() < f32::EPSILON);
  assert!((style.core.line_height_factor - default.core.line_height_factor).abs() < f32::EPSILON);
  assert!(style.core.background_color.is_none());
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
  let toml = "font_size = \"15pt\"\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  assert!((style.core.font_size.to_pt() - 15.0).abs() < f32::EPSILON);
  let default = Style::default();
  assert!((style.core.line_height_factor - default.core.line_height_factor).abs() < f32::EPSILON);
}

#[test]
fn parse_style_rejects_color_array() {
  // Arrange: 旧形式の RGB 配列は破壊的に廃止された
  let toml = "background_color = [204, 179, 153]\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}

#[test]
fn parse_style_accepts_color_hex_string() {
  // Arrange
  let toml = "background_color = \"#cc9966\"\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  let color = style.core.background_color.expect("background_color should be Some");
  assert_eq!(color.rgb(), [0xcc, 0x99, 0x66]);
}

#[test]
fn parse_style_fails_on_unknown_top_level_key() {
  // Arrange: typo
  let toml = "font_sze = \"15pt\"\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}

#[test]
fn parse_style_fails_on_unknown_nested_key() {
  // Arrange: typo inside [heading.chapter]
  let toml = "[heading.chapter]\nfont_sze = \"30pt\"\n";

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
fn read_style_fails_on_nonexistent_path() {
  // Arrange
  let path = PathBuf::from("/nonexistent/style.toml");

  // Act
  let result = read_style(Some(path.as_path()));

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ReadFile { .. })));
}

#[test]
fn parse_style_reads_minimal_fixture() {
  // Arrange: 部分指定のみの代表 fixture
  let toml = include_str!("fixtures/minimal.toml");

  // Act
  let style = parse_style(toml, "minimal.toml").unwrap();

  // Assert: 指定したフィールドだけ上書きされ、他はデフォルト
  assert!((style.core.font_size.to_pt() - 14.0).abs() < f32::EPSILON);
  assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
}
