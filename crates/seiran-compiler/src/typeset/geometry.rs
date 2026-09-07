//! 版面の幾何 — `config.toml`（用紙寸法）× `style.toml`（`[page]` の余白・`[columns]`）の
//! 横断バリデーションと、そこから確定する版面 [`PreparedGeometry`] の構築。
//!
//! どちらの設定 module にも属さない（片方だけでは判定できない）ので、この制約を不変条件として
//! 使う組版側が所有する（#351）。余白単体の不正（負値）は style の値検証が持ち、ここが持つのは
//! 「用紙寸法と突き合わせないと判定できない制約」だけ（#389）。
//! [`PreparedGeometry::prepare`] を呼ぶのは入力読込（`compiler::input::load`）で、組版に入る前に
//! 不正な組み合わせを弾く。検証を通った版面（本文幅・段幅・本文 / 前付け / 後付けのページ幾何）は
//! 戻り値として下流へ渡り、`typeset::pagination` はそれを読むだけで再計算しない（#533）。

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  color::Color, failures::Failures, length::Length, project::config::ProjectConfig, style::Style,
  typeset::breaking::PageGeometry,
};

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
/// `(text_width - (num_columns - 1) * column_gap) / num_columns`。
/// [`PreparedGeometry::prepare`] と `typeset::breaking::break_pages` の実配置が同じ式を参照する。
#[must_use]
pub(super) fn column_width(text_width: Length, num_columns: usize, column_gap: Length) -> Length {
  let count = num_columns.max(1);
  #[expect(
    clippy::cast_precision_loss,
    reason = "段数は実用上 1〜2 で、桁あふれ・精度低下・切り捨てが起きる桁数にならない"
  )]
  let n = count as f32;
  #[expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    reason = "段数は実用上 1〜2 で、桁あふれ・精度低下・切り捨てが起きる桁数にならない"
  )]
  let gaps = (count - 1) as i32;
  return (text_width - column_gap * gaps) / n;
}

/// 横断検証を通った版面。
///
/// 本文幅・本文の 1 段あたりの幅・本文 / 前付け / 後付けのページ幾何を確定値として持つ。
/// フィールドは module 非公開で、構築経路は [`PreparedGeometry::prepare`] だけ — 「検証を通って
/// いない版面が組版へ流れない」ことを型で保証する（`Failures` と同じ方針、#533）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreparedGeometry {
  /// 版面幅（段組み前）= `pdf.width - page.margin_left - page.margin_right`
  text_width: Length,
  /// 本文の 1 段あたりの幅（画像サイズ解決に使う）
  body_column_width: Length,
  /// 本文のページジオメトリ（`style.columns.count` 段）
  body_geometry: PageGeometry,
  /// 前付けのページジオメトリ（常に 1 段・下端揃えなし）
  front_geometry: PageGeometry,
  /// 後付け（索引）のページジオメトリ（`style.index.column_count` 段・下端揃えなし）
  back_geometry: PageGeometry,
}

impl PreparedGeometry {
  /// [`ProjectConfig`]（用紙寸法）と [`Style`]（`[page]` の余白・`[columns]`）の横断制約を検証し、
  /// 通ったときだけ確定した版面を返します。
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
  /// ページ幾何を組み立てるのは 3 件すべてが通った後だけで、不正な組み合わせから版面が生まれる
  /// 経路は存在しません。
  ///
  /// # Errors
  ///
  /// 上記のいずれかに違反した場合、違反ぶんの [`LayoutValidationError`] を持つ非空集合を返します。
  pub(crate) fn prepare(config: &ProjectConfig, style: &Style) -> Result<Self, Failures<LayoutValidationError>> {
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
    let text_width = config.pdf.width - horizontal;
    let num_columns = style.columns.count as usize;
    let column_gap = style.columns.gap;
    let body_column_width = column_width(text_width, num_columns, column_gap);
    if horizontal >= config.pdf.width {
      errors.push(LayoutValidationError::HorizontalMarginsExceedPageWidth {
        margin_left: style.page.margin_left.to_pt(),
        margin_right: style.page.margin_right.to_pt(),
        total: horizontal.to_pt(),
        page_width: config.pdf.width.to_pt(),
      });
    } else if !body_column_width.is_positive() {
      errors.push(LayoutValidationError::InvalidColumnWidth {
        text_width: text_width.to_pt(),
        num_columns,
        column_gap: column_gap.to_pt(),
      });
    }

    if let Some(failures) = Failures::from_vec(errors) {
      return Err(failures);
    }

    let (body_geometry, front_geometry, back_geometry) = build_page_geometries(config, style, num_columns, column_gap);
    return Ok(Self {
      text_width,
      body_column_width,
      body_geometry,
      front_geometry,
      back_geometry,
    });
  }

  /// 版面幅（段組み前）を返す。
  #[must_use]
  pub(super) fn text_width(&self) -> Length { return self.text_width; }

  /// 本文の 1 段あたりの幅を返す。
  #[must_use]
  pub(super) fn body_column_width(&self) -> Length { return self.body_column_width; }

  /// 本文のページジオメトリを返す。
  #[must_use]
  pub(super) fn body_geometry(&self) -> &PageGeometry { return &self.body_geometry; }

  /// 前付けのページジオメトリを返す。
  #[must_use]
  pub(super) fn front_geometry(&self) -> &PageGeometry { return &self.front_geometry; }

  /// 後付け（索引）のページジオメトリを返す。
  #[must_use]
  pub(super) fn back_geometry(&self) -> &PageGeometry { return &self.back_geometry; }
}

/// 本文・前付け・後付けのページジオメトリを組み立てる。
///
/// 段数・段間以外は本文の値を共有する。
fn build_page_geometries(
  config: &ProjectConfig,
  style: &Style,
  body_columns: usize,
  column_gap: Length,
) -> (PageGeometry, PageGeometry, PageGeometry) {
  let body_geometry = PageGeometry {
    content_origin_x: style.page.margin_left,
    margin_top: style.page.margin_top,
    page_limit: config.pdf.height - style.page.margin_bottom,
    default_font_size: style.text.font_size,
    line_height_factor: style.text.line_height_factor,
    table_cell_padding: style.table.cell_padding,
    num_columns: body_columns,
    column_gap,
    flush_bottom: style.page.flush_bottom,
    footnote_top_margin: style.footnote.top_margin,
    footnote_rule_length: style.footnote.rule_length,
    footnote_rule_thickness: style.footnote.rule_thickness,
    footnote_rule_color: style.footnote.rule_color.map(Color::rgb),
    footnote_rule_gap: style.footnote.rule_gap,
    table_rule_thickness: style.table.rule_thickness,
    table_rule_color: style.table.rule_color.map(Color::rgb),
    background_color: style.background_color.map(Color::rgb),
  };
  let front_geometry = PageGeometry {
    num_columns: 1,
    column_gap: Length::ZERO,
    flush_bottom: false,
    ..body_geometry
  };
  let back_geometry = PageGeometry {
    num_columns: usize::from(style.index.column_count),
    flush_bottom: false,
    ..body_geometry
  };
  return (body_geometry, front_geometry, back_geometry);
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{LayoutValidationError, Length, PreparedGeometry, ProjectConfig, Style, column_width};
  use crate::project::{
    FilesystemProjectSource, PathResolver, ProjectPath,
    config::{
      self,
      test_support::{make_font_sections, valid_output_section, valid_pdf_section},
    },
  };

  /// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します
  /// （旧 `crates/config/tests/common/mod.rs` の統合テスト用ヘルパ、`project/config.rs` の
  /// `mod tests` にある同名ヘルパの複製 — `PreparedGeometry::prepare` は `config::load` の実結果に
  /// 対して検証するため、こちらでも同じ実ファイルシステム経由のフィクスチャ生成が要る）。
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
    let (config, _) = config::load(&source, &ProjectPath::new(&config_path), &PathResolver::new(&base_dir)).unwrap();
    return (tempdir, config);
  }

  fn pt(value: f32) -> Length { return Length::pt(value); }

  fn close(a: Length, b: f32) -> bool { return (a.to_pt() - b).abs() < 0.01; }

  #[test]
  fn column_width_helper_divides_text_width() {
    // 本文幅 100pt を 2 段（段間 10pt）と 1 段に割ったときの 1 段幅
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
  fn prepare_accepts_default_config_and_style() {
    let (_tempdir, config) = read_test_config();
    let style = test_style(50.0, 50.0, 50.0, 50.0);

    assert!(PreparedGeometry::prepare(&config, &style).is_ok());
  }

  #[test]
  fn prepare_rejects_column_gap_wider_than_text_width() {
    // Arrange
    let (_tempdir, config) = read_test_config();
    let mut style = test_style(50.0, 50.0, 50.0, 50.0);
    style.columns.count = 2;
    style.columns.gap = config.pdf.width;

    // Act
    let failures = PreparedGeometry::prepare(&config, &style).unwrap_err();

    // Assert
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty());
    assert!(matches!(first, LayoutValidationError::InvalidColumnWidth { num_columns: 2, .. }));
  }

  #[test]
  fn prepare_rejects_vertical_margins_exceeding_page_height() {
    // Arrange — 用紙高 842pt に対し上下合計 900pt
    let (_tempdir, config) = read_test_config();
    let style = test_style(450.0, 450.0, 50.0, 50.0);

    // Act
    let failures = PreparedGeometry::prepare(&config, &style).unwrap_err();

    // Assert
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty());
    assert!(matches!(first, LayoutValidationError::VerticalMarginsExceedPageHeight { .. }));
  }

  #[test]
  fn prepare_rejects_horizontal_margins_exceeding_page_width() {
    // Arrange — 用紙幅 595pt に対し左右合計 600pt
    let (_tempdir, config) = read_test_config();
    let style = test_style(50.0, 50.0, 300.0, 300.0);

    // Act
    let failures = PreparedGeometry::prepare(&config, &style).unwrap_err();

    // Assert — 左右余白だけで本文幅が尽きているので、派生する段幅エラーは重ねない
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty(), "段幅エラーを重ねないはず: {rest:?}");
    assert!(matches!(first, LayoutValidationError::HorizontalMarginsExceedPageWidth { .. }));
  }

  #[test]
  fn prepare_reports_vertical_and_horizontal_violations_in_input_order() {
    // Arrange — 上下・左右がともに不正
    let (_tempdir, config) = read_test_config();
    let style = test_style(450.0, 450.0, 300.0, 300.0);

    // Act
    let failures = PreparedGeometry::prepare(&config, &style).unwrap_err();

    // Assert — 縦 → 横の論理順で 2 件
    let (first, rest) = failures.into_parts();
    assert!(matches!(first, LayoutValidationError::VerticalMarginsExceedPageHeight { .. }));
    assert_eq!(rest.len(), 1);
    assert!(matches!(rest[0], LayoutValidationError::HorizontalMarginsExceedPageWidth { .. }));
  }

  #[test]
  fn prepare_derives_text_width_and_body_column_width() {
    // Arrange — 用紙幅から左右 50pt ずつを引いた本文幅を、段間 15pt の 2 段に割る
    let (_tempdir, config) = read_test_config();
    let mut style = test_style(50.0, 50.0, 50.0, 50.0);
    style.columns.count = 2;
    style.columns.gap = pt(15.0);

    // Act
    let prepared = PreparedGeometry::prepare(&config, &style).unwrap();

    // Assert — fixture の用紙幅から導出して、式そのものを固定する
    let expected_text_width = config.pdf.width.to_pt() - 100.0;
    assert!(close(prepared.text_width(), expected_text_width), "本文幅: {:?}", prepared.text_width());
    assert!(
      close(prepared.body_column_width(), (expected_text_width - 15.0) / 2.0),
      "段幅: {:?}",
      prepared.body_column_width()
    );
  }

  #[test]
  fn prepare_derives_front_and_back_geometry_from_body() {
    // Arrange — 本文 2 段・下端揃えあり、索引 3 段
    let (_tempdir, config) = read_test_config();
    let mut style = test_style(50.0, 50.0, 50.0, 50.0);
    style.columns.count = 2;
    style.page.flush_bottom = true;
    style.index.column_count = 3;

    // Act
    let prepared = PreparedGeometry::prepare(&config, &style).unwrap();

    // Assert — 前付けは常に 1 段・段間 0・下端揃えなし、後付けは索引の段数で下端揃えなし
    assert_eq!(prepared.body_geometry().num_columns, 2);
    assert!(prepared.body_geometry().flush_bottom, "本文は style の flush_bottom に従うはず");
    assert_eq!(prepared.front_geometry().num_columns, 1);
    assert_eq!(prepared.front_geometry().column_gap, Length::ZERO);
    assert!(!prepared.front_geometry().flush_bottom, "前付けは下端揃えしないはず");
    assert_eq!(prepared.back_geometry().num_columns, 3);
    assert!(!prepared.back_geometry().flush_bottom, "後付けは下端揃えしないはず");
    assert_eq!(
      prepared.back_geometry().column_gap,
      prepared.body_geometry().column_gap,
      "後付けの段間は本文と同じはず"
    );
    assert_eq!(
      prepared.front_geometry().margin_top,
      prepared.body_geometry().margin_top,
      "段数・段間・下端揃え以外は本文の値を共有するはず"
    );
  }
}
