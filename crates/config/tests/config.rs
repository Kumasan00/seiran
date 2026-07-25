//! `read_config` の公開 API を経由するエンドツーエンド統合テスト。

use config::{
  Config, ConfigValidationError, ReadConfigError, TextDirection, read_config,
  test_support::{font_sections_with_serif_extra, make_font_sections, valid_output_section, valid_pdf_section},
};
use model::FontType;

mod common;
use common::setup_config;

#[test]
fn read_config_succeeds_with_valid_config() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n[document]\ntitle = \"Test Doc\"\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.output.name, "test_doc");
  assert_eq!(config.document.title.as_deref(), Some("Test Doc"));
  assert_eq!(config.sources.len(), 1);
  assert_eq!(config.font_configs.iter().count(), 19);
  assert_eq!(config.font_configs.get(FontType::Serif).font_name, "font_serif");
  assert!(config.pdf.show_bookmarks);
  assert_eq!(config.image.max_dpi, 300);
  assert!(config.image.downsample);
}

#[test]
fn read_config_reads_image_overrides() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}[image]\nmax_dpi = 150\ndownsample = false\n\n{}",
      valid_output_section("test", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.image.max_dpi, 150);
  assert!(!config.image.downsample);
}

#[test]
fn read_config_respects_show_bookmarks_false() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n{}[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
       margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\
       show_bookmarks = false\n\n{}",
      valid_output_section("test", output_dir),
      make_font_sections(font_path),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert!(!config.pdf.show_bookmarks);
}

#[test]
fn read_config_fails_on_nonexistent_font_path() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|_font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test", output_dir),
      valid_pdf_section(),
      make_font_sections("/nonexistent/path/to/font.ttf"),
    );
  });

  // Act
  let result = read_config(&config_path);

  // Assert
  let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
    panic!("expected MultipleValidationErrors, got {result:?}");
  };
  assert!(errors.iter().all(|error| matches!(error, ConfigValidationError::FontPathResolution { .. })));
  assert_eq!(errors.len(), 19);
}

#[test]
fn read_config_fails_on_nonexistent_source_path() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, _source_path| {
    return format!(
      "sources = [\"/nonexistent/source.sei\"]\n\n{}{}{}",
      valid_output_section("test", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });

  // Act
  let result = read_config(&config_path);

  // Assert
  let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
    panic!("expected MultipleValidationErrors, got {result:?}");
  };
  assert!(errors.iter().any(|error| matches!(error, ConfigValidationError::SourcePathResolution { .. })));
}

#[test]
fn read_config_uses_current_dir_when_output_dir_omitted() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, _output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n[output]\nname = \"out\"\n\n{}{}",
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });
  let expected_output_dir = std::env::current_dir().unwrap().canonicalize().unwrap();

  // Act
  let config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.output.output_dir, expected_output_dir);
  assert_eq!(config.output.pdf_path(), expected_output_dir.join("out.pdf"));
}

#[test]
fn read_config_preserves_user_script_tag_case() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "script = \"Latn\"";
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    );
  });

  let config: Config = read_config(&config_path).unwrap();

  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.script, Some(*b"Latn"));
}

#[test]
fn read_config_builds_language_string_with_ot_language_suffix() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "language = \"ja\"\nscript = \"kana\"\not_language = \"JAN\"";
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.language.as_deref(), Some("ja-x-hbotJAN"));
  assert_eq!(serif.script, Some(*b"kana"));
  assert_eq!(serif.ot_language_tag, Some(*b"JAN "));
}

#[test]
fn read_config_builds_language_string_with_und_base_when_only_ot_language() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "script = \"latn\"\not_language = \"ENG\"";
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.language.as_deref(), Some("und-x-hbotENG"));
  assert_eq!(serif.ot_language_tag, Some(*b"ENG "));
}

#[test]
fn read_config_preserves_user_direction() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "direction = \"right-to-left\"";
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.direction, Some(TextDirection::RightToLeft));
}

#[test]
fn read_config_preserves_document_language_and_keywords() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n[document]\nlanguage = \"ja\"\nkeywords = [\"組版\", \"PDF\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.document.language.as_deref(), Some("ja"));
  assert_eq!(config.document.keywords.as_deref(), Some(&["組版".to_string(), "PDF".to_string()][..]));
}

#[test]
fn read_config_keeps_document_language_and_keywords_none_when_omitted() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.document.language, None);
  assert_eq!(config.document.keywords, None);
}

#[test]
fn read_config_keeps_direction_none_when_omitted() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  for font_type in FontType::ALL {
    assert_eq!(config.font_configs.get(font_type).direction, None, "{font_type:?}");
  }
}
