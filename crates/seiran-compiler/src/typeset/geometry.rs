//! 版面の幾何 — `config.toml`（用紙・余白）× `style.toml`（`[columns]`）の横断バリデーションと、
//! 段組み設定から導出する 1 段あたりの幅の計算。
//!
//! どちらの設定 module にも属さない（片方だけでは判定できない）ので、この制約を不変条件として
//! 使う組版側が所有する（#351）。[`validate_layout`] を呼ぶのは入力読込（`compiler::input::load`）で、
//! 組版に入る前に不正な組み合わせを弾く。

use miette::Diagnostic;
use thiserror::Error;

use crate::{length::Length, project::config::ProjectConfig, style::Style};

/// config × style 横断バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum LayoutValidationError {
  /// 段組み設定により 1 段あたりの幅が 0 以下になった場合
  #[error(
    "段組みの 1 段あたりの幅が 0 以下になりました（本文幅 {text_width:.1}pt / 段数 {num_columns} / 段間 {column_gap:.1}pt）。"
  )]
  #[diagnostic(
    code(typeset::geometry::invalid_columns),
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

/// 本文幅 `text_width` を `num_columns` 段に分けたときの 1 段あたりの幅（pt）を返す。
///
/// `(text_width - (num_columns - 1) * column_gap) / num_columns`。[`validate_layout`] と
/// `typeset::pagination::context` / `typeset::breaking::break_pages` の実配置が同じ式を参照する。
#[must_use]
pub(super) fn column_width(text_width: Length, num_columns: usize, column_gap: Length) -> Length {
  let count = num_columns.max(1);
  // 段数は実用上 1〜2。桁あふれ・精度低下・切り捨てが起きる桁数にはならない
  #[allow(clippy::cast_precision_loss)]
  let n = count as f32;
  #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
  let gaps = (count - 1) as i32;
  return (text_width - column_gap * gaps) / n;
}

/// [`ProjectConfig`]（用紙・余白）と [`Style`]（`[columns]`）の横断制約を検証します。
///
/// 本文幅（`pdf.width - margin.left - margin.right`）を `style.columns` の段数・段間で割った
/// 1 段あたりの幅が非正の場合にエラーを返します。
///
/// # Errors
///
/// 1 段あたりの幅が 0 以下の場合 [`LayoutValidationError::InvalidColumnWidth`] を返します。
pub(crate) fn validate_layout(config: &ProjectConfig, style: &Style) -> Result<(), LayoutValidationError> {
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

  use super::{LayoutValidationError, Length, ProjectConfig, Style, column_width, validate_layout};
  use crate::project::{
    FilesystemProjectSource,
    config::{
      self,
      test_support::{make_font_sections, valid_output_section, valid_pdf_section},
    },
  };

  /// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します
  /// （旧 `crates/config/tests/common/mod.rs` の統合テスト用ヘルパ、`project/config.rs` の
  /// `mod tests` にある同名ヘルパの複製 — `validate_layout` は `config::load` の実結果に対して
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

  fn read_test_config() -> (tempfile::TempDir, ProjectConfig) {
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
    let (config, _) = config::load(&source, &config_path, &base_dir).unwrap();
    return (tempdir, config);
  }

  fn pt(value: f32) -> Length { return Length::pt(value); }

  fn close(a: Length, b: f32) -> bool { return (a.to_pt() - b).abs() < 0.01; }

  #[test]
  fn column_width_helper_divides_text_width() {
    // Arrange / Act / Assert — 本文幅 100pt を 2 段（段間 10pt）と 1 段に割ったときの 1 段幅
    assert!(close(column_width(pt(100.0), 2, pt(10.0)), 45.0));
    assert!(close(column_width(pt(100.0), 1, pt(18.0)), 100.0));
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
