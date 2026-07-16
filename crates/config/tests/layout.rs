//! `config` × `style` 横断バリデーション（[`validate_layout`]）の統合テスト。
//!
//! `read_config` / `parse_style` の公開 API を経由して実際のパイプラインと同じ形の
//! [`Config`] / [`Style`] を組み立て、`validate_layout` に渡す。

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
  let config = read_config(&config_path).unwrap();
  return (tempdir, config);
}

#[test]
fn validate_layout_accepts_default_config_and_style() {
  // Arrange: `valid_pdf_section` は A4・50pt 余白、既定スタイルは単段
  let (_tempdir, config) = read_test_config();
  let style = Style::default();

  // Act / Assert
  assert!(validate_layout(&config, &style).is_ok());
}

#[test]
fn validate_layout_rejects_column_gap_wider_than_text_width() {
  // Arrange: 2 段組みで段間を本文幅（595 - 50*2 = 495pt）以上に広げる
  let (_tempdir, config) = read_test_config();
  let mut style = Style::default();
  style.columns.count = 2;
  style.columns.gap = config.pdf.width;

  // Act
  let error = validate_layout(&config, &style).unwrap_err();

  // Assert
  assert!(matches!(error, LayoutValidationError::InvalidColumnWidth { num_columns: 2, .. }));
}
