//! `style.toml`（見た目）のデータモデル・既定値・読込・検証
//!
//! 言語設計原則 P10 が区別する 2 概念のうち「種類ごとの見た目」を所有する crate root の module。
//! 物理・実体・メタデータ（`config.toml`）は [`crate::project::config`] の所有で、両者は互いを知らない
//! （どちらか一方だけでは判定できない横断制約は [`crate::typeset::validate_layout`] が持つ）。
//! CSL ファイル自体の読込は行わない — 引用箇所の存在が確定するまで遅延させるため、ここは
//! `csl_path` / `locale_path` の正規化と存在確認までで止める。

mod caption;
mod columns;
mod counter;
mod error;
mod figure;
mod footnote;
mod heading;
mod hyperref;
mod index;
mod list;
mod math;
mod number_style;
mod page;
mod page_numbering;
mod placeholder;
mod quote;
mod reference;
mod running;
mod table;
mod text;
mod theorem;
mod title_page;
mod toc;

use std::path::Path;

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

// `compiler::error::CompileError` が `#[from]` で運ぶために名指しする読込エラー。
pub(crate) use crate::style::error::ReadStyleError;
// module root が再エクスポートするのは、`style` の外から実際に名指しされる名前だけ。
// `Style` の内部フィールド型としてしか現れないサブスタイル型（`FigureStyle` / `HeadingStyle` 等）は
// 下の非公開 `use` に留め、`crate::style::FigureStyle` という到達経路を作らない。
// `NestedOrderedFormat` / `NumberStyle` のように利用側が `#[cfg(test)] mod tests` だけの名前があるため
// `unused_imports` を許可する（非公開 module からの再エクスポートは本体ビルドでは未使用に見える）。
#[allow(unused_imports)]
pub use crate::style::{
  caption::CaptionStyle,
  counter::{CounterName, CounterStyle, Counters},
  footnote::{FootnoteNumbering, FootnoteStyle},
  list::NestedOrderedFormat,
  math::{Alignment, MathScriptStyle, NumberSide},
  number_style::NumberStyle,
  page_numbering::PageNumbering,
  running::RunningContentStyle,
  text::TextAlignment,
  theorem::{TheoremReset, TheoremStyle},
  title_page::TitlePageStyle,
  toc::TocStyle,
};
// `Style` のフィールド型と、この module 内でのみ名指しする型。`style` の子 module からは
// `crate::style::<Type>` で参照できる（非公開 `use` の名前も子孫からは見える）。
use crate::style::{
  columns::ColumnsStyle,
  error::StyleValidationError,
  figure::FigureStyle,
  heading::{HeadingStyle, HeadingStyles},
  hyperref::HyperrefStyle,
  index::IndexStyle,
  list::ListStyle,
  math::MathStyle,
  page::PageStyle,
  quote::QuoteStyle,
  reference::ReferenceStyle,
  table::TableStyle,
  text::TextBlockStyle,
  theorem::{TheoremClass, Theorems},
};
use crate::{
  color::Color,
  document::HeadingLevel,
  failures::Failures,
  project::{ProjectPath, ProjectSource},
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct Style {
  /// 背景色。`None` は背景描画なし
  #[garde(skip)]
  pub background_color: Option<Color>,
  /// 見出し全 6 レベルのスタイル
  #[garde(skip)]
  pub heading: HeadingStyles,
  /// 本文段落のスタイル
  #[garde(dive)]
  pub text: TextBlockStyle,
  /// 段組み（1 段 / 2 段切替）のスタイル
  #[garde(dive)]
  pub columns: ColumnsStyle,
  /// ページ組版の挙動（下端揃え等）のスタイル
  #[garde(dive)]
  pub page: PageStyle,
  /// リストのスタイル
  #[garde(dive)]
  pub list: ListStyle,
  /// 引用ブロック（quote / quotation）のスタイル
  #[garde(dive)]
  pub quote: QuoteStyle,
  /// 表のスタイル
  #[garde(dive)]
  pub table: TableStyle,
  /// 図フロートのスタイル
  #[garde(dive)]
  pub figure: FigureStyle,
  /// 脚注のスタイル
  #[garde(dive)]
  pub footnote: FootnoteStyle,
  /// 数式のスタイル（`[math.script]` スクリプト / `[math.block]` 表示数式ブロックのレイアウト）
  #[garde(dive)]
  pub math: MathStyle,
  /// カウンタ定義テーブル（`[counters.<name>]`、固定 9 種）
  #[garde(dive)]
  pub counters: Counters,
  /// 定理クラス定義テーブル（`[theorems.<class>]`、固定 10 種）
  #[garde(skip)]
  pub theorems: Theorems,
  /// ページ番号のスタイル（前付け＝ローマ数字 / 本文＝算用数字）
  #[garde(dive)]
  pub page_numbering: PageNumbering,
  /// ヘッダー（ページ上端の走り文）のスタイル
  #[garde(dive)]
  pub header: RunningContentStyle,
  /// フッター（ページ下端の走り文）のスタイル
  #[garde(dive)]
  pub footer: RunningContentStyle,
  /// 参考文献セクションのスタイル
  #[garde(dive)]
  pub reference: ReferenceStyle,
  /// ハイパーリンク（hyperref 相当）のスタイル
  #[garde(dive)]
  pub hyperref: HyperrefStyle,
  /// タイトルページ（`\maketitle` 相当）のスタイル
  #[garde(dive)]
  pub title_page: TitlePageStyle,
  /// 目次（table of contents）のスタイル
  #[garde(dive)]
  pub toc: TocStyle,
  /// 巻末索引のスタイル
  #[garde(dive)]
  pub index: IndexStyle,
}

impl Default for Style {
  fn default() -> Self {
    return Self {
      background_color: None,
      heading: HeadingStyles::default(),
      text: TextBlockStyle::default(),
      columns: ColumnsStyle::default(),
      page: PageStyle::default(),
      list: ListStyle::default(),
      quote: QuoteStyle::default(),
      table: TableStyle::default(),
      figure: FigureStyle::default(),
      footnote: FootnoteStyle::default(),
      math: MathStyle::default(),
      counters: Counters::default(),
      theorems: Theorems::default(),
      page_numbering: PageNumbering::default(),
      header: RunningContentStyle::default(),
      footer: RunningContentStyle::default(),
      reference: ReferenceStyle::default(),
      hyperref: HyperrefStyle::default(),
      title_page: TitlePageStyle::default(),
      toc: TocStyle::default(),
      index: IndexStyle::default(),
    };
  }
}

impl Style {
  /// 指定された見出しレベルの [`HeadingStyle`] への不変参照を返す
  #[must_use]
  pub fn heading(&self, level: HeadingLevel) -> &HeadingStyle { return &self.heading[level]; }

  /// 指定された定理クラスのスタイル定義への不変参照を返す（10 種固定のため必ず存在する）。
  #[must_use]
  pub fn theorem(&self, class: TheoremClass) -> &TheoremStyle { return self.theorems.get(class); }
}

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はファイルを読み込まずに [`Style::default`] を返します。
/// `path` が相対パスの場合は `base_dir` を基準に解決します。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・値検証・参照ファイルのパス解決に失敗した場合はエラーを返します。
// 設定ファイルは 1 回しか読まないため、Result サイズを最適化する価値が低い。
#[allow(clippy::result_large_err)]
pub(crate) fn load(
  source: &dyn ProjectSource,
  path: Option<&Path>,
  base_dir: &Path,
) -> Result<Style, Failures<ReadStyleError>> {
  let Some(path) = path else {
    info!("スタイル設定ファイルが指定されていないため、デフォルト値を使用します");
    return Ok(Style::default());
  };
  let path_str = path.display().to_string();
  debug!(style_path = %path_str, "スタイル設定ファイルの読み込みを開始します");

  let joined = if path.is_absolute() {
    path.to_path_buf()
  } else {
    base_dir.join(path)
  };
  let content = source.read_text(&ProjectPath::new(&joined)).map_err(|source| {
    return Failures::single(ReadStyleError::ReadFile {
      path: path_str.clone(),
      source: source.into_io(),
    });
  })?;

  let mut style = parse(&content, &path_str)?;

  if let Some(failures) = validation_failures(resolve_reference_paths(&mut style.reference, source, base_dir)) {
    return Err(failures);
  }

  info!(
    font_size_pt = style.text.font_size.to_pt(),
    line_height_factor = style.text.line_height_factor,
    "スタイル設定ファイルの読み込みが完了しました"
  );
  return Ok(style);
}

/// TOML 文字列を [`Style`] にパースし、値検証とロケールコードの正規化まで実行します（I/O なし）。
///
/// # Errors
///
/// TOML 解析または値検証に失敗した場合はエラーを返します。
#[allow(clippy::result_large_err)]
pub(crate) fn parse(content: &str, source_path: &str) -> Result<Style, Failures<ReadStyleError>> {
  let mut style: Style = toml::from_str(content).map_err(|source| {
    let src = NamedSource::new(source_path, content.to_string());
    let span = source.span().map_or_else(
      || return SourceSpan::new(0.into(), 0),
      |range| return SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    return Failures::single(ReadStyleError::ParseToml { src, span, source });
  })?;
  if let Err(errors) = validate_values(&style)
    && let Some(failures) = validation_failures(errors)
  {
    return Err(failures);
  }
  style.reference.normalize();
  return Ok(style);
}

/// 値検証の違反列を、1 件ずつ独立した leaf 診断として運ぶ非空集合へ変換する（空なら `None`）。
#[allow(clippy::result_large_err)]
fn validation_failures(errors: Vec<StyleValidationError>) -> Option<Failures<ReadStyleError>> {
  return Failures::from_vec(errors.into_iter().map(ReadStyleError::from).collect());
}

/// [`Style`] の値検証を実行します（I/O なし）。
fn validate_values(style: &Style) -> Result<(), Vec<StyleValidationError>> {
  let mut errors: Vec<StyleValidationError> = Vec::new();

  if let Err(report) = style.validate() {
    errors.extend(report.iter().map(|(path, error)| {
      return StyleValidationError::Field {
        path: path.to_string(),
        message: error.to_string(),
      };
    }));
  }

  for (level, heading) in style.heading.iter_with_level() {
    if let Err(report) = heading.validate() {
      errors.extend(report.iter().map(|(path, error)| {
        return StyleValidationError::Field {
          path: format!("heading.{}.{path}", level.command_name()),
          message: error.to_string(),
        };
      }));
    }
  }

  for (class, theorem) in style.theorems.iter_with_class() {
    if let Err(report) = theorem.validate() {
      errors.extend(report.iter().map(|(path, error)| {
        return StyleValidationError::Field {
          path: format!("theorems.{}.{path}", class.as_str()),
          message: error.to_string(),
        };
      }));
    }
  }

  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
}

/// `style.reference` の CSL 関連パス（`csl_path` / `locale_path`）を `base_dir` 基準の絶対パスへ
/// 解決し、`source.exists` でファイルの存在を同時に検証します（I/O フェーズ）。
fn resolve_reference_paths(
  reference: &mut ReferenceStyle,
  source: &dyn ProjectSource,
  base_dir: &Path,
) -> Vec<StyleValidationError> {
  let mut errors: Vec<StyleValidationError> = Vec::new();

  if let Some(path) = reference.csl_path.take() {
    let joined = if path.is_absolute() {
      path.clone()
    } else {
      base_dir.join(&path)
    };
    if source.exists(&ProjectPath::new(&joined)) {
      reference.csl_path = Some(joined);
    } else {
      errors.push(StyleValidationError::CslPathResolution {
        path: joined.display().to_string(),
      });
    }
  }

  if let Some(path) = reference.locale_path.take() {
    let joined = if path.is_absolute() {
      path.clone()
    } else {
      base_dir.join(&path)
    };
    if source.exists(&ProjectPath::new(&joined)) {
      reference.locale_path = Some(joined);
    } else {
      errors.push(StyleValidationError::LocalePathResolution {
        path: joined.display().to_string(),
      });
    }
  }

  return errors;
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use garde::Validate;
  use tempfile::NamedTempFile;

  use crate::{
    document::HeadingLevel,
    length::Length,
    project::{FilesystemProjectSource, MemoryProjectSource},
    style::{ReadStyleError, ReferenceStyle, Style, StyleValidationError, load, resolve_reference_paths},
  };

  #[test]
  fn resolve_reference_paths_makes_csl_path_absolute() {
    // Arrange
    let file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    let source = FilesystemProjectSource::new();
    let mut reference = ReferenceStyle {
      csl_path: Some(file.path().to_path_buf()),
      ..ReferenceStyle::default()
    };

    // Act
    let errors = resolve_reference_paths(&mut reference, &source, Path::new("/unused"));

    // Assert
    assert!(errors.is_empty(), "実在パスはエラーにならないはず: {errors:?}");
    assert!(reference.csl_path.expect("csl_path は残るはず").is_absolute());
  }

  #[test]
  fn resolve_reference_paths_reports_missing_files() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let mut reference = ReferenceStyle {
      csl_path: Some(PathBuf::from("/nonexistent/style.csl")),
      locale_path: Some(PathBuf::from("/nonexistent/locale.xml")),
      ..ReferenceStyle::default()
    };

    // Act
    let errors = resolve_reference_paths(&mut reference, &source, Path::new("/unused"));

    // Assert
    assert_eq!(errors.len(), 2, "csl_path / locale_path 双方が報告されるはず: {errors:?}");
    assert!(errors.iter().any(|e| matches!(e, StyleValidationError::CslPathResolution { .. })));
    assert!(errors.iter().any(|e| matches!(e, StyleValidationError::LocalePathResolution { .. })));
  }

  #[test]
  fn resolve_reference_paths_skips_none() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let mut reference = ReferenceStyle::default();

    // Act
    let errors = resolve_reference_paths(&mut reference, &source, Path::new("/unused"));

    // Assert
    assert!(errors.is_empty());
  }

  #[test]
  fn load_reads_through_project_source() {
    // Arrange
    let source = MemoryProjectSource::new().with_text("/project/style.toml", "");
    let path = PathBuf::from("style.toml");

    // Act
    let style = load(&source, Some(&path), Path::new("/project")).expect("空の TOML は既定値になるはず");

    // Assert
    assert_eq!(style.text.font_size, Style::default().text.font_size);
  }

  #[test]
  fn load_reports_missing_csl_path_without_touching_real_disk() {
    // Arrange
    let toml = "[reference]\ncsl_path = \"missing.csl\"\n";
    let source = MemoryProjectSource::new().with_text("/project/style.toml", toml);
    let path = PathBuf::from("style.toml");

    // Act
    let result = load(&source, Some(&path), Path::new("/project"));

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::Validation(StyleValidationError::CslPathResolution { .. }))
    ));
  }

  #[test]
  fn load_aggregates_both_missing_csl_and_locale_paths() {
    // Arrange — csl_path / locale_path のどちらも欠落している場合、片方だけで
    // fail-fast せず、両方が同じ Vec に集約されることを検証する
    let toml = "[reference]\ncsl_path = \"missing.csl\"\nlocale_path = \"missing.xml\"\n";
    let source = MemoryProjectSource::new().with_text("/project/style.toml", toml);
    let path = PathBuf::from("style.toml");

    // Act
    let result = load(&source, Some(&path), Path::new("/project"));

    // Assert
    let Err(failures) = result else {
      panic!("2 件の検証エラーを期待");
    };
    let errors: Vec<&ReadStyleError> = failures.iter().collect();
    assert!(
      errors
        .iter()
        .any(|e| matches!(e, ReadStyleError::Validation(StyleValidationError::CslPathResolution { .. })))
    );
    assert!(
      errors
        .iter()
        .any(|e| matches!(e, ReadStyleError::Validation(StyleValidationError::LocalePathResolution { .. })))
    );
  }

  #[test]
  fn validate_accepts_default() {
    assert!(Style::default().validate().is_ok());
  }

  #[test]
  fn validate_dives_into_text_font_size() {
    let mut style = Style::default();
    style.text.font_size = Length::pt(0.0);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_text_line_height_factor() {
    let mut style = Style::default();
    style.text.line_height_factor = 0.0;
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_nested_table_rule_thickness() {
    let mut style = Style::default();
    style.table.rule_thickness = Length::pt(-0.1);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_quote_indent() {
    let mut style = Style::default();
    style.quote.indent = Length::pt(-0.1);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_dives_into_counters_display_name() {
    let mut style = Style::default();
    style.counters.figure.display_name = String::new();
    assert!(style.validate().is_err());
  }

  #[test]
  fn rejects_renamed_top_level_text_keys() {
    assert!(toml::from_str::<Style>("font_size = \"12pt\"\n").is_err());
    assert!(toml::from_str::<Style>("line_height_factor = 1.2\n").is_err());
  }

  #[test]
  fn default_heading_has_descending_font_size() {
    let style = Style::default();
    let part = style.heading(HeadingLevel::Part).font_size.to_pt();
    let chapter = style.heading(HeadingLevel::Chapter).font_size.to_pt();
    let section = style.heading(HeadingLevel::Section).font_size.to_pt();
    let subparagraph = style.heading(HeadingLevel::Subparagraph).font_size.to_pt();
    assert!(part > chapter, "Part should be larger than Chapter: {part} vs {chapter}");
    assert!(chapter > section, "Chapter should be larger than Section: {chapter} vs {section}");
    assert!(section > subparagraph, "Section should be larger than Subparagraph");
  }
}

/// TOML パース系の統合テスト（旧 `crates/config/tests/parse.rs`）。
///
/// `mod tests` と同居させず別 module にしているのは、両ファイルとも `fn dummy_source()` という
/// 同名ヘルパを独立に定義していたため（`mod tests` へ素直に統合すると名前衝突する）。
#[cfg(test)]
mod parse_tests {
  use super::{ReadStyleError, Style, TheoremClass, load, parse};
  use crate::{document::HeadingLevel, project::FilesystemProjectSource};

  fn dummy_source() -> &'static str { return "test.toml"; }

  #[test]
  fn load_returns_default_when_path_is_none() {
    // Arrange
    let source = FilesystemProjectSource::new();

    // Act
    let style = load(&source, None, std::path::Path::new(".")).unwrap();

    // Assert
    let default = Style::default();
    assert!((style.text.font_size.to_pt() - default.text.font_size.to_pt()).abs() < f32::EPSILON);
    assert!((style.text.line_height_factor - default.text.line_height_factor).abs() < f32::EPSILON);
    assert!(style.background_color.is_none());
    assert_eq!(style.heading(HeadingLevel::Part).format, default.heading(HeadingLevel::Part).format);
  }

  #[test]
  fn parse_overrides_heading_section_format() {
    // Arrange
    let toml = "[heading.section]\nformat = \"§ {number} {title}\"\n";

    // Act
    let style = parse(toml, dummy_source()).unwrap();

    // Assert
    assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
    let default = Style::default();
    assert_eq!(style.heading(HeadingLevel::Chapter).format, default.heading(HeadingLevel::Chapter).format);
  }

  #[test]
  fn parse_overrides_only_specified_fields() {
    // Arrange
    let toml = "[text]\nfont_size = \"15pt\"\n";

    // Act
    let style = parse(toml, dummy_source()).unwrap();

    // Assert
    assert!((style.text.font_size.to_pt() - 15.0).abs() < f32::EPSILON);
    let default = Style::default();
    assert!((style.text.line_height_factor - default.text.line_height_factor).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_overrides_columns() {
    // Arrange
    let toml = "[columns]\ncount = 2\ngap = \"24pt\"\n";

    // Act
    let style = parse(toml, dummy_source()).unwrap();

    // Assert
    assert_eq!(style.columns.count, 2);
    assert!((style.columns.gap.to_pt() - 24.0).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_columns_defaults_to_single_column() {
    // Arrange / Act
    let style = parse("", dummy_source()).unwrap();

    // Assert
    assert_eq!(style.columns.count, 1);
    assert!((style.columns.gap.to_pt() - 18.0).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_enables_flush_bottom() {
    // Arrange
    let toml = "[page]\nflush_bottom = true\n";

    // Act
    let style = parse(toml, dummy_source()).unwrap();

    // Assert
    assert!(style.page.flush_bottom);
  }

  #[test]
  fn parse_flush_bottom_defaults_to_disabled() {
    // Arrange / Act
    let style = parse("", dummy_source()).unwrap();

    // Assert
    assert!(!style.page.flush_bottom);
  }

  #[test]
  fn parse_fails_on_unknown_page_key() {
    // Arrange
    let toml = "[page]\nflush_botom = true\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn parse_fails_on_unknown_columns_key() {
    // Arrange
    let toml = "[columns]\ncont = 2\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_rejects_color_array() {
    // Arrange
    let toml = "background_color = [204, 179, 153]\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_accepts_color_hex_string() {
    // Arrange
    let toml = "background_color = \"#cc9966\"\n";

    // Act
    let style = parse(toml, dummy_source()).unwrap();

    // Assert
    let color = style.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [0xcc, 0x99, 0x66]);
  }

  #[test]
  fn parse_overrides_header_and_footer() {
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
    let style = parse(toml, dummy_source()).unwrap();

    // Assert
    assert_eq!(style.header.right, "{page} / {pages}");
    assert!((style.header.font_size.to_pt() - 9.0).abs() < f32::EPSILON);
    assert_eq!(style.header.font_kind, crate::document::FontKind::SansSerif);
    assert!((style.header.rule_thickness.to_pt() - 0.5).abs() < f32::EPSILON);
    assert_eq!(style.header.rule_color.map(crate::color::Color::rgb), Some([0x33, 0x33, 0x33]));
    assert_eq!(style.footer.center, "{title}");
    assert!(style.footer.left.is_empty());
    assert!(!style.header.is_empty());
  }

  #[test]
  fn parse_fails_on_unknown_header_key() {
    // Arrange
    let toml = "[header]\nrght = \"{page}\"\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_unknown_top_level_key() {
    // Arrange
    let toml = "font_sze = \"15pt\"\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_unknown_nested_key() {
    // Arrange
    let toml = "[heading.chapter]\nfont_sze = \"30pt\"\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_invalid_toml_syntax() {
    // Arrange
    let toml = "font_size = \nthis is not valid toml";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn load_fails_on_nonexistent_path() {
    // Arrange
    let path = std::path::PathBuf::from("/nonexistent/style.toml");
    let source = FilesystemProjectSource::new();
    let base_dir = path.parent().expect("フィクスチャパスは親ディレクトリを持つはず");

    // Act
    let result = load(&source, Some(path.as_path()), base_dir);

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ReadFile { .. })
    ));
  }

  #[test]
  fn parse_reads_minimal_fixture() {
    // Arrange
    let toml = include_str!("style/fixtures/minimal.toml");

    // Act
    let style = parse(toml, "minimal.toml").unwrap();

    // Assert
    assert!((style.text.font_size.to_pt() - 14.0).abs() < f32::EPSILON);
    assert_eq!(style.heading(HeadingLevel::Section).format, "§ {number} {title}");
  }

  #[test]
  fn parse_overrides_theorem_class_partially() {
    // Arrange
    let toml = "[theorems.lemma]\ndisplay_name = \"補題\"\n";

    // Act
    let style = parse(toml, dummy_source()).unwrap();

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
  fn parse_fails_on_unknown_theorem_class() {
    // Arrange
    let toml = "[theorems.conjecture]\ndisplay_name = \"Conjecture\"\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_unknown_theorem_field() {
    // Arrange
    let toml = "[theorems.theorem]\ndispl_name = \"Theorem\"\n";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }
}

/// 検証系の統合テスト（旧 `crates/config/tests/validate.rs`）。
///
/// `parse_tests` と同様、独立の `fn dummy_source()` を持つため別 module にしている。
#[cfg(test)]
mod validate_tests {
  use super::{Failures, ReadStyleError, Style, StyleValidationError, parse};

  fn dummy_source() -> &'static str { return "test.toml"; }

  fn expect_validation_errors(result: Result<Style, Failures<ReadStyleError>>) -> Vec<StyleValidationError> {
    let Err(failures) = result else {
      panic!("検証エラーを期待");
    };
    return failures
      .into_iter()
      .map(|error| {
        let ReadStyleError::Validation(validation) = error else {
          panic!("Validation を期待: {error:?}");
        };
        return validation;
      })
      .collect();
  }

  fn paths(errors: &[StyleValidationError]) -> Vec<&str> {
    return errors
      .iter()
      .map(|error| match error {
        StyleValidationError::Field { path, .. }
        | StyleValidationError::CslPathResolution { path, .. }
        | StyleValidationError::LocalePathResolution { path, .. } => return path.as_str(),
      })
      .collect();
  }

  #[test]
  fn parse_collects_multiple_validation_errors() {
    // Arrange
    let toml = "[text]\nfont_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    let paths = paths(&errors);
    assert!(paths.contains(&"text.font_size"));
    assert!(paths.contains(&"heading.chapter.font_size"));
  }

  #[test]
  fn rejects_three_columns() {
    // Arrange
    let toml = "[columns]\ncount = 3\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    assert!(paths(&errors).contains(&"columns.count"));
  }

  #[test]
  fn rejects_zero_columns() {
    // Arrange
    let toml = "[columns]\ncount = 0\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    assert!(paths(&errors).contains(&"columns.count"));
  }

  #[test]
  fn rejects_negative_column_gap() {
    // Arrange
    let toml = "[columns]\ngap = \"-1pt\"\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    assert!(paths(&errors).contains(&"columns.gap"));
  }

  #[test]
  fn reports_nested_theorem_style_validation_error_with_path() {
    // Arrange
    let toml = "[theorems.theorem.style]\ntop_margin = \"-1pt\"\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    let paths = paths(&errors);
    assert!(
      paths.contains(&"theorems.theorem.style.top_margin"),
      "expected theorems.theorem.style.top_margin in {paths:?}"
    );
  }

  #[test]
  fn reports_theorem_empty_display_name_with_path() {
    // Arrange
    let toml = "[theorems.lemma]\ndisplay_name = \"\"\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    let paths = paths(&errors);
    assert!(paths.contains(&"theorems.lemma.display_name"), "expected theorems.lemma.display_name in {paths:?}");
  }

  #[test]
  fn rejects_unknown_counter_name_at_parse_time() {
    // Arrange
    let toml = "
[counters.custom]
display_name = \"Custom\"
number_format = \"{chapter}.{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = []
";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(
      matches!(result.as_ref().map_err(|failures| return failures.first()), Err(ReadStyleError::ParseToml { .. })),
      "unknown counter name should be rejected at TOML parse time, got {result:?}"
    );
  }

  #[test]
  fn reports_unknown_placeholders_across_fields_together() {
    // Arrange
    let toml = "
[heading.section]
format = \"{nubmer} {title}\"

[footer]
center = \"{pagee}\"
";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    let paths = paths(&errors);
    assert!(paths.contains(&"heading.section.format"), "expected heading.section.format in {paths:?}");
    assert!(paths.contains(&"footer.center"), "expected footer.center in {paths:?}");
  }

  #[test]
  fn placeholder_error_message_names_the_offending_token() {
    // Arrange
    let toml = "[math.block]\ntag_format = \"({num})\"\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    let message = errors
      .iter()
      .find_map(|error| match error {
        StyleValidationError::Field { path, message } if path == "math.block.tag_format" => {
          return Some(message.as_str());
        },
        _ => return None,
      })
      .expect("math.block.tag_format のエラーがあるはず");
    assert!(message.contains("{num}"), "メッセージに {{num}} を含むべき: {message}");
  }

  #[test]
  fn rejects_unknown_counter_placeholder_in_number_format() {
    // Arrange
    let toml = "[counters.section]\ndisplay_name = \"Section\"\nnumber_format = \"{chaptr}.{n}\"\nnumber_style = \"arabic\"\nref_format = \"{display_name} {number}\"\nresets = []\n";

    // Act
    let errors = expect_validation_errors(parse(toml, dummy_source()));

    // Assert
    let paths = paths(&errors);
    assert!(
      paths.contains(&"counters.section.number_format"),
      "expected counters.section.number_format in {paths:?}"
    );
  }

  #[test]
  fn rejects_unknown_reset_target_at_parse_time() {
    // Arrange
    let toml = "
[counters.chapter]
display_name = \"Chapter\"
number_format = \"{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = [\"nonexistent\"]
";

    // Act
    let result = parse(toml, dummy_source());

    // Assert
    assert!(
      matches!(result.as_ref().map_err(|failures| return failures.first()), Err(ReadStyleError::ParseToml { .. })),
      "unknown reset target should be rejected at TOML parse time, got {result:?}"
    );
  }
}
