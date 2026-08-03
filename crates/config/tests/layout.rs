//! `config` × `style` 横断バリデーション（[`validate_layout`]）の統合テスト。

use config::{
  Config, LayoutValidationError, Style, read_config,
  test_support::{make_font_sections, valid_output_section, valid_pdf_section},
  validate_layout,
};

mod common;
use common::setup_config;

fn read_test_config() -> (tempfile::TempDir, Config) {
  let (tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    return format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    );
  });
  let source = config::FilesystemProjectSource::new();
  let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
  let config = read_config(&source, &config_path, &base_dir).unwrap();
  return (tempdir, config);
}

#[test]
fn validate_layout_accepts_default_config_and_style() {
  // Arrange
  let (_tempdir, config) = read_test_config();
  let style = Style::default();

  // Act / Assert
  assert!(validate_layout(&config, &style).is_ok());
}

#[test]
fn validate_layout_rejects_column_gap_wider_than_text_width() {
  // Arrange
  let (_tempdir, config) = read_test_config();
  let mut style = Style::default();
  style.columns.count = 2;
  style.columns.gap = config.pdf.width;

  // Act
  let error = validate_layout(&config, &style).unwrap_err();

  // Assert
  assert!(matches!(error, LayoutValidationError::InvalidColumnWidth { num_columns: 2, .. }));
}
