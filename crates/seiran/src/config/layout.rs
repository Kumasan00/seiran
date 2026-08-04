//! `config`（用紙・余白）× `style`（`[columns]`）の横断バリデーション

use miette::Diagnostic;
use model::column_width;
use thiserror::Error;

use crate::config::{Config, Style};

/// config × style 横断バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum LayoutValidationError {
  /// 段組み設定により 1 段あたりの幅が 0 以下になった場合
  #[error(
    "段組みの 1 段あたりの幅が 0 以下になりました（本文幅 {text_width:.1}pt / 段数 {num_columns} / 段間 {column_gap:.1}pt）。"
  )]
  #[diagnostic(
    code(config::validation::invalid_columns),
    help(
      "style.toml の [columns].gap を小さくするか、count を減らしてください。または config.toml の用紙幅を広げる・左右余白を狭めて本文幅を確保してください。"
    )
  )]
  InvalidColumnWidth {
    /// 本文幅（pt）
    text_width: f32,
    /// 段数
    num_columns: usize,
    /// 段間（pt）
    column_gap: f32,
  },
}

/// [`Config`]（用紙・余白）と [`Style`]（`[columns]`）の横断制約を検証します。
///
/// 本文幅（`pdf.width - margin.left - margin.right`）を `style.columns` の段数・段間で割った
/// 1 段あたりの幅が非正の場合にエラーを返します。
///
/// # Errors
///
/// 1 段あたりの幅が 0 以下の場合 [`LayoutValidationError::InvalidColumnWidth`] を返します。
pub fn validate_layout(config: &Config, style: &Style) -> Result<(), LayoutValidationError> {
  let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
  let num_columns = style.columns.count as usize;
  let column_gap = style.columns.gap;

  if !column_width(text_width, num_columns, column_gap).is_positive() {
    return Err(LayoutValidationError::InvalidColumnWidth {
      text_width: text_width.to_pt(),
      num_columns,
      column_gap: column_gap.to_pt(),
    });
  }
  return Ok(());
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{Config, LayoutValidationError, Style, validate_layout};
  use crate::config::{
    config_toml::{
      read_config,
      test_support::{make_font_sections, valid_output_section, valid_pdf_section},
    },
    project_source::FilesystemProjectSource,
  };

  /// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します
  /// （旧 `crates/config/tests/common/mod.rs` の統合テスト用ヘルパ、`config_toml.rs` の
  /// `mod tests` にある同名ヘルパの複製 — `validate_layout` は `read_config` の実結果に対して
  /// 検証するため、こちらでも同じ実ファイルシステム経由のフィクスチャ生成が要る）。
  fn setup_config(build_toml: impl FnOnce(&str, &str, &str) -> String) -> (tempfile::TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().expect("一時ディレクトリを作成できるはず");
    let font_path = tempdir.path().join("dummy.ttf");
    std::fs::write(&font_path, b"").expect("ダミーフォントを書き込めるはず");
    let source_path = tempdir.path().join("source.sei");
    std::fs::write(&source_path, b"").expect("ダミーソースを書き込めるはず");
    let output_dir = tempdir.path().join("output");
    let config_path = tempdir.path().join("config.toml");
    let toml_text =
      build_toml(font_path.to_str().unwrap(), output_dir.to_str().unwrap(), source_path.to_str().unwrap());
    std::fs::write(&config_path, toml_text).expect("config.toml を書き込めるはず");
    return (tempdir, config_path);
  }

  fn read_test_config() -> (tempfile::TempDir, Config) {
    let (tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });
    let source = FilesystemProjectSource::new();
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
}
