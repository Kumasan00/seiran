//! 版面の幾何 — `config.toml`（用紙寸法）× `style.toml`（`[page]` の余白・`[columns]`）の
//! 横断バリデーションと、段組み設定から導出する 1 段あたりの幅の計算。
//!
//! どちらの設定 module にも属さない（片方だけでは判定できない）ので、この制約を不変条件として
//! 使う組版側が所有する（#351）。余白単体の不正（負値）は style の値検証が持ち、ここが持つのは
//! 「用紙寸法と突き合わせないと判定できない制約」だけ（#389）。[`validate_layout`] を呼ぶのは
//! 入力読込（`compiler::input::load`）で、組版に入る前に不正な組み合わせを弾く。

use miette::Diagnostic;
use thiserror::Error;

use crate::{failures::Failures, length::Length, project::config::ProjectConfig, style::Style};

/// config × style 横断バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum LayoutValidationError {
  /// 上下余白の合計が用紙高以上で、本文の高さが残らない場合
  #[error(
    "上下余白の合計 {total:.1}pt が用紙高 {page_height:.1}pt 以上で、本文の高さが残りません（上 {margin_top:.1}pt / 下 {margin_bottom:.1}pt）。"
  )]
  #[diagnostic(
    code(typeset::geometry::vertical_margins),
    help(
      "style.toml の [page].margin_top / margin_bottom を小さくするか、config.toml の [pdf].height を大きくしてください。"
    )
  )]
  VerticalMarginsExceedPageHeight {
    /// 上余白（pt）
    margin_top: f32,
    /// 下余白（pt）
    margin_bottom: f32,
    /// 上下余白の合計（pt）
    total: f32,
    /// 用紙高（pt）
    page_height: f32,
  },
  /// 左右余白の合計が用紙幅以上で、本文の幅が残らない場合
  #[error(
    "左右余白の合計 {total:.1}pt が用紙幅 {page_width:.1}pt 以上で、本文の幅が残りません（左 {margin_left:.1}pt / 右 {margin_right:.1}pt）。"
  )]
  #[diagnostic(
    code(typeset::geometry::horizontal_margins),
    help(
      "style.toml の [page].margin_left / margin_right を小さくするか、config.toml の [pdf].width を大きくしてください。"
    )
  )]
  HorizontalMarginsExceedPageWidth {
    /// 左余白（pt）
    margin_left: f32,
    /// 右余白（pt）
    margin_right: f32,
    /// 左右余白の合計（pt）
    total: f32,
    /// 用紙幅（pt）
    page_width: f32,
  },
  /// 段組み設定により 1 段あたりの幅が 0 以下になった場合
  #[error(
    "段組みの 1 段あたりの幅が 0 以下になりました（本文幅 {text_width:.1}pt / 段数 {num_columns} / 段間 {column_gap:.1}pt）。"
  )]
  #[diagnostic(
    code(typeset::geometry::invalid_columns),
    help(
      "style.toml の [columns].gap を小さくするか、count を減らしてください。または style.toml の [page].margin_left / margin_right を狭める・config.toml の [pdf].width を広げて本文幅を確保してください。"
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

/// [`ProjectConfig`]（用紙寸法）と [`Style`]（`[page]` の余白・`[columns]`）の横断制約を検証します。
///
/// 次の 3 つを独立に検査し、違反を**入力の論理順（縦 → 横 → 段幅）** で集約します。
///
/// 1. 上下余白の合計が用紙高未満であること
/// 2. 左右余白の合計が用紙幅未満であること
/// 3. 本文幅（`pdf.width - page.margin_left - page.margin_right`）を `style.columns` の段数・段間で
///    割った 1 段あたりの幅が正であること
///
/// 3 は 2 が通っているときだけ検査します（左右余白だけで本文幅が尽きているときに、そこから
/// 派生するだけの段幅エラーを重ねてもユーザーの修正先が増えないため）。
///
/// # Errors
///
/// 上記のいずれかに違反した場合、違反ぶんの [`LayoutValidationError`] を持つ非空集合を返します。
pub(crate) fn validate_layout(config: &ProjectConfig, style: &Style) -> Result<(), Failures<LayoutValidationError>> {
  let mut errors: Vec<LayoutValidationError> = Vec::new();

  let vertical = style.page.margin_top + style.page.margin_bottom;
  if vertical >= config.pdf.height {
    errors.push(LayoutValidationError::VerticalMarginsExceedPageHeight {
      margin_top: style.page.margin_top.to_pt(),
      margin_bottom: style.page.margin_bottom.to_pt(),
      total: vertical.to_pt(),
      page_height: config.pdf.height.to_pt(),
    });
  }

  let horizontal = style.page.margin_left + style.page.margin_right;
  if horizontal >= config.pdf.width {
    errors.push(LayoutValidationError::HorizontalMarginsExceedPageWidth {
      margin_left: style.page.margin_left.to_pt(),
      margin_right: style.page.margin_right.to_pt(),
      total: horizontal.to_pt(),
      page_width: config.pdf.width.to_pt(),
    });
  } else {
    let text_width = config.pdf.width - horizontal;
    let num_columns = style.columns.count as usize;
    let column_gap = style.columns.gap;
    if !column_width(text_width, num_columns, column_gap).is_positive() {
      errors.push(LayoutValidationError::InvalidColumnWidth {
        text_width: text_width.to_pt(),
        num_columns,
        column_gap: column_gap.to_pt(),
      });
    }
  }

  if let Some(failures) = Failures::from_vec(errors) {
    return Err(failures);
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

  /// 用紙（`valid_pdf_section` の A4 = 595×842pt）に収まる余白を明示した style を作る。
  ///
  /// 余白は style が所有するため、横断検証のテストは config 側ではなくここを動かして
  /// 版面の組み合わせを作る（#389）。
  fn test_style(margin_top: f32, margin_bottom: f32, margin_left: f32, margin_right: f32) -> Style {
    let mut style = Style::default();
    style.page.margin_top = pt(margin_top);
    style.page.margin_bottom = pt(margin_bottom);
    style.page.margin_left = pt(margin_left);
    style.page.margin_right = pt(margin_right);
    return style;
  }

  #[test]
  fn validate_layout_accepts_default_config_and_style() {
    // Arrange
    let (_tempdir, config) = read_test_config();
    let style = test_style(50.0, 50.0, 50.0, 50.0);

    // Act / Assert
    assert!(validate_layout(&config, &style).is_ok());
  }

  #[test]
  fn validate_layout_rejects_column_gap_wider_than_text_width() {
    // Arrange
    let (_tempdir, config) = read_test_config();
    let mut style = test_style(50.0, 50.0, 50.0, 50.0);
    style.columns.count = 2;
    style.columns.gap = config.pdf.width;

    // Act
    let failures = validate_layout(&config, &style).unwrap_err();

    // Assert
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty());
    assert!(matches!(first, LayoutValidationError::InvalidColumnWidth { num_columns: 2, .. }));
  }

  #[test]
  fn validate_layout_rejects_vertical_margins_exceeding_page_height() {
    // Arrange — 用紙高 842pt に対し上下合計 900pt
    let (_tempdir, config) = read_test_config();
    let style = test_style(450.0, 450.0, 50.0, 50.0);

    // Act
    let failures = validate_layout(&config, &style).unwrap_err();

    // Assert
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty());
    assert!(matches!(first, LayoutValidationError::VerticalMarginsExceedPageHeight { .. }));
  }

  #[test]
  fn validate_layout_rejects_horizontal_margins_exceeding_page_width() {
    // Arrange — 用紙幅 595pt に対し左右合計 600pt
    let (_tempdir, config) = read_test_config();
    let style = test_style(50.0, 50.0, 300.0, 300.0);

    // Act
    let failures = validate_layout(&config, &style).unwrap_err();

    // Assert — 左右余白だけで本文幅が尽きているので、派生する段幅エラーは重ねない
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty(), "段幅エラーを重ねないはず: {rest:?}");
    assert!(matches!(first, LayoutValidationError::HorizontalMarginsExceedPageWidth { .. }));
  }

  #[test]
  fn validate_layout_reports_vertical_and_horizontal_violations_in_input_order() {
    // Arrange — 上下・左右がともに不正
    let (_tempdir, config) = read_test_config();
    let style = test_style(450.0, 450.0, 300.0, 300.0);

    // Act
    let failures = validate_layout(&config, &style).unwrap_err();

    // Assert — 縦 → 横の論理順で 2 件
    let (first, rest) = failures.into_parts();
    assert!(matches!(first, LayoutValidationError::VerticalMarginsExceedPageHeight { .. }));
    assert_eq!(rest.len(), 1);
    assert!(matches!(rest[0], LayoutValidationError::HorizontalMarginsExceedPageWidth { .. }));
  }
}
