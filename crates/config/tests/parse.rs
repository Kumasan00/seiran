//! TOML パース系の統合テスト。

use std::path::{Path, PathBuf};

use config::{ReadStyleError, Style, TheoremClass, parse_style, read_style};
use model::HeadingLevel;

fn dummy_source() -> &'static str { return "test.toml"; }

#[test]
fn read_style_returns_default_when_path_is_none() {
  // Arrange
  let source = config::FilesystemProjectSource::new();

  // Act
  let style = read_style(&source, None, Path::new(".")).unwrap();

  // Assert
  let default = Style::default();
  assert!((style.text.font_size.to_pt() - default.text.font_size.to_pt()).abs() < f32::EPSILON);
  assert!((style.text.line_height_factor - default.text.line_height_factor).abs() < f32::EPSILON);
  assert!(style.background_color.is_none());
  assert_eq!(style.heading(HeadingLevel::Part).format, default.heading(HeadingLevel::Part).format);
}

#[test]
fn parse_style_overrides_heading_section_format() {
  // Arrange
  let toml = "[heading.section]\nformat = \"§ {number} {title}\"\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
  let default = Style::default();
  assert_eq!(style.heading(HeadingLevel::Chapter).format, default.heading(HeadingLevel::Chapter).format);
}

#[test]
fn parse_style_overrides_only_specified_fields() {
  // Arrange
  let toml = "[text]\nfont_size = \"15pt\"\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  assert!((style.text.font_size.to_pt() - 15.0).abs() < f32::EPSILON);
  let default = Style::default();
  assert!((style.text.line_height_factor - default.text.line_height_factor).abs() < f32::EPSILON);
}

#[test]
fn parse_style_overrides_columns() {
  // Arrange
  let toml = "[columns]\ncount = 2\ngap = \"24pt\"\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  assert_eq!(style.columns.count, 2);
  assert!((style.columns.gap.to_pt() - 24.0).abs() < f32::EPSILON);
}

#[test]
fn parse_style_columns_defaults_to_single_column() {
  // Arrange / Act
  let style = parse_style("", dummy_source()).unwrap();

  // Assert
  assert_eq!(style.columns.count, 1);
  assert!((style.columns.gap.to_pt() - 18.0).abs() < f32::EPSILON);
}

#[test]
fn parse_style_enables_flush_bottom() {
  // Arrange
  let toml = "[page]\nflush_bottom = true\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  assert!(style.page.flush_bottom);
}

#[test]
fn parse_style_flush_bottom_defaults_to_disabled() {
  // Arrange / Act
  let style = parse_style("", dummy_source()).unwrap();

  // Assert
  assert!(!style.page.flush_bottom);
}

#[test]
fn parse_style_fails_on_unknown_page_key() {
  // Arrange
  let toml = "[page]\nflush_botom = true\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(result.is_err());
}

#[test]
fn parse_style_fails_on_unknown_columns_key() {
  // Arrange
  let toml = "[columns]\ncont = 2\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}

#[test]
fn parse_style_rejects_color_array() {
  // Arrange
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
  // Arrange
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

  // Assert
  assert_eq!(style.header.right, "{page} / {pages}");
  assert!((style.header.font_size.to_pt() - 9.0).abs() < f32::EPSILON);
  assert_eq!(style.header.font_kind, model::FontKind::SansSerif);
  assert!((style.header.rule_thickness.to_pt() - 0.5).abs() < f32::EPSILON);
  assert_eq!(style.header.rule_color.map(model::Color::rgb), Some([0x33, 0x33, 0x33]));
  assert_eq!(style.footer.center, "{title}");
  assert!(style.footer.left.is_empty());
  assert!(!style.header.is_empty());
}

#[test]
fn parse_style_fails_on_unknown_header_key() {
  // Arrange
  let toml = "[header]\nrght = \"{page}\"\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}

#[test]
fn parse_style_fails_on_unknown_top_level_key() {
  // Arrange
  let toml = "font_sze = \"15pt\"\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}

#[test]
fn parse_style_fails_on_unknown_nested_key() {
  // Arrange
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
  let source = config::FilesystemProjectSource::new();
  let base_dir = path.parent().expect("フィクスチャパスは親ディレクトリを持つはず");

  // Act
  let result = read_style(&source, Some(path.as_path()), base_dir);

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ReadFile { .. })));
}

#[test]
fn parse_style_reads_minimal_fixture() {
  // Arrange
  let toml = include_str!("fixtures/minimal.toml");

  // Act
  let style = parse_style(toml, "minimal.toml").unwrap();

  // Assert
  assert!((style.text.font_size.to_pt() - 14.0).abs() < f32::EPSILON);
  assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
}

#[test]
fn parse_style_overrides_theorem_class_partially() {
  // Arrange
  let toml = "[theorems.lemma]\ndisplay_name = \"補題\"\n";

  // Act
  let style = parse_style(toml, dummy_source()).unwrap();

  // Assert
  assert_eq!(style.theorem(TheoremClass::Lemma).display_name, "補題");
  assert_eq!(style.theorem(TheoremClass::Lemma).counter, "theorem");
  let default = Style::default();
  assert_eq!(
    style.theorem(TheoremClass::Theorem).display_name,
    default.theorem(TheoremClass::Theorem).display_name
  );
}

#[test]
fn parse_style_fails_on_unknown_theorem_class() {
  // Arrange
  let toml = "[theorems.conjecture]\ndisplay_name = \"Conjecture\"\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}

#[test]
fn parse_style_fails_on_unknown_theorem_field() {
  // Arrange
  let toml = "[theorems.theorem]\ndispl_name = \"Theorem\"\n";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(matches!(result, Err(ReadStyleError::ParseToml { .. })));
}
