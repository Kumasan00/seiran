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
  assert!((style.font_size.to_pt() - default.font_size.to_pt()).abs() < f32::EPSILON);
  assert!((style.line_height_factor - default.line_height_factor).abs() < f32::EPSILON);
  assert!(style.background_color.is_none());
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
  assert!((style.font_size.to_pt() - 15.0).abs() < f32::EPSILON);
  let default = Style::default();
  assert!((style.line_height_factor - default.line_height_factor).abs() < f32::EPSILON);
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
  let color = style.background_color.expect("background_color should be Some");
  assert_eq!(color.rgb(), [0xcc, 0x99, 0x66]);
}

#[test]
fn parse_style_overrides_header_and_footer() {
  // Arrange: [header] / [footer] のスロットと見た目を上書き
  let toml = concat!(
    "[header]\n",
    "right = \"{page} / {pages}\"\n",
    "font_size = \"9pt\"\n",
    "font_kind = \"sans_serif\"\n",
    "rule_thickness = \"0.5pt\"\n",
    "rule_color = \"#333333\"\n",
    "[footer]\n",
    "center = \"{title}\"\n",
  );

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert: header は指定値、footer は center のみ指定で他はデフォルト
  assert_eq!(style.header.right, "{page} / {pages}");
  assert!((style.header.font_size.to_pt() - 9.0).abs() < f32::EPSILON);
  assert_eq!(style.header.font_kind, types::FontKind::SansSerif);
  assert!((style.header.rule_thickness.to_pt() - 0.5).abs() < f32::EPSILON);
  assert_eq!(style.header.rule_color.map(types::Color::rgb), Some([0x33, 0x33, 0x33]));
  assert_eq!(style.footer.center, "{title}");
  assert!(style.footer.left.is_empty());
  // 既定では header/footer は空（描画なし）
  assert!(!style.header.is_empty());
}

#[test]
fn parse_style_fails_on_unknown_header_key() {
  // Arrange: [header] 内の typo は deny_unknown_fields で拒否
  let toml = "[header]\nrght = \"{page}\"\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
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
  assert!((style.font_size.to_pt() - 14.0).abs() < f32::EPSILON);
  assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
}
