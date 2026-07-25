//! TOML スタイル設定ファイルのパース・検証モジュール

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

use std::{fs, path::Path};

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use model::{Color, HeadingLevel};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

pub use crate::style::{
  caption::CaptionStyle,
  columns::ColumnsStyle,
  counter::{CounterName, CounterStyle, Counters},
  error::{ReadStyleError, StyleValidationError},
  figure::FigureStyle,
  footnote::{FootnoteNumbering, FootnoteStyle},
  heading::{HeadingStyle, HeadingStyles, default_for_level},
  hyperref::HyperrefStyle,
  index::IndexStyle,
  list::{ListStyle, NestedOrderedFormat},
  math::{Alignment, MathBlockStyle, MathScriptStyle, MathStyle, NumberSide},
  number_style::NumberStyle,
  page::PageStyle,
  page_numbering::PageNumbering,
  quote::QuoteStyle,
  reference::ReferenceStyle,
  running::RunningContentStyle,
  table::TableStyle,
  text::TextBlockStyle,
  theorem::{TheoremClass, TheoremPresentation, TheoremReset, TheoremStyle, Theorems, default_for_class},
  title_page::TitlePageStyle,
  toc::TocStyle,
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

  /// 指定された名前のカウンタ定義への不変参照を返す（9 種固定のため必ず存在する）。
  #[must_use]
  pub fn counter(&self, name: CounterName) -> &CounterStyle { return self.counters.get(name); }

  /// 指定された定理クラスのスタイル定義への不変参照を返す（10 種固定のため必ず存在する）。
  #[must_use]
  pub fn theorem(&self, class: TheoremClass) -> &TheoremStyle { return self.theorems.get(class); }
}

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はファイルを読み込まずに [`Style::default`] を返します。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・値検証・参照ファイルのパス解決に失敗した場合はエラーを返します。
// 設定ファイルは 1 回しか読まないため、Result サイズを最適化する価値が低い。
#[allow(clippy::result_large_err)]
pub fn read_style(path: Option<&Path>) -> Result<Style, ReadStyleError> {
  let Some(path) = path else {
    info!("スタイル設定ファイルが指定されていないため、デフォルト値を使用します");
    return Ok(Style::default());
  };
  let path_str = path.display().to_string();
  debug!(style_path = %path_str, "スタイル設定ファイルの読み込みを開始します");

  let content = fs::read_to_string(path).map_err(|source| {
    return ReadStyleError::ReadFile {
      path: path_str.clone(),
      source,
    };
  })?;

  let mut style = parse_style(&content, &path_str)?;

  let errors = resolve_reference_paths(&mut style.reference);
  if !errors.is_empty() {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
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
pub fn parse_style(content: &str, source_path: &str) -> Result<Style, ReadStyleError> {
  let mut style: Style = toml::from_str(content).map_err(|source| {
    let src = NamedSource::new(source_path, content.to_string());
    let span = source.span().map_or_else(
      || return SourceSpan::new(0.into(), 0),
      |range| return SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    return ReadStyleError::ParseToml { src, span, source };
  })?;
  if let Err(errors) = validate_values(&style) {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }
  style.reference.normalize();
  return Ok(style);
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

/// `style.reference` の CSL 関連パス（`csl_path` / `locale_path`）を `canonicalize` で絶対パスへ
/// 正規化し、ファイルの存在を同時に検証します（I/O フェーズ）。
fn resolve_reference_paths(reference: &mut ReferenceStyle) -> Vec<StyleValidationError> {
  let mut errors: Vec<StyleValidationError> = Vec::new();

  if let Some(path) = reference.csl_path.take() {
    match path.canonicalize() {
      Ok(canonical) => reference.csl_path = Some(canonical),
      Err(source) => errors.push(StyleValidationError::CslPathResolution {
        path: path.display().to_string(),
        source,
      }),
    }
  }

  if let Some(path) = reference.locale_path.take() {
    match path.canonicalize() {
      Ok(canonical) => reference.locale_path = Some(canonical),
      Err(source) => errors.push(StyleValidationError::LocalePathResolution {
        path: path.display().to_string(),
        source,
      }),
    }
  }

  return errors;
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use garde::Validate;
  use model::{Color, HeadingLevel, length::Length};
  use tempfile::NamedTempFile;

  use crate::style::{CounterName, ReferenceStyle, Style, StyleValidationError, TheoremClass, resolve_reference_paths};

  #[test]
  fn resolve_reference_paths_makes_csl_path_absolute() {
    // Arrange
    let file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    let mut reference = ReferenceStyle {
      csl_path: Some(file.path().to_path_buf()),
      ..ReferenceStyle::default()
    };

    // Act
    let errors = resolve_reference_paths(&mut reference);

    // Assert
    assert!(errors.is_empty(), "実在パスはエラーにならないはず: {errors:?}");
    assert!(reference.csl_path.expect("csl_path は残るはず").is_absolute());
  }

  #[test]
  fn resolve_reference_paths_reports_missing_files() {
    // Arrange
    let mut reference = ReferenceStyle {
      csl_path: Some(PathBuf::from("/nonexistent/style.csl")),
      locale_path: Some(PathBuf::from("/nonexistent/locale.xml")),
      ..ReferenceStyle::default()
    };

    // Act
    let errors = resolve_reference_paths(&mut reference);

    // Assert
    assert_eq!(errors.len(), 2, "csl_path / locale_path 双方が報告されるはず: {errors:?}");
    assert!(errors.iter().any(|e| matches!(e, StyleValidationError::CslPathResolution { .. })));
    assert!(errors.iter().any(|e| matches!(e, StyleValidationError::LocalePathResolution { .. })));
  }

  #[test]
  fn resolve_reference_paths_skips_none() {
    // Arrange / Act / Assert
    let mut reference = ReferenceStyle::default();
    let errors = resolve_reference_paths(&mut reference);
    assert!(errors.is_empty());
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
  fn heading_accessor_returns_correct_level() {
    let style = Style::default();
    assert!(style.heading(HeadingLevel::Part).font_size > style.heading(HeadingLevel::Section).font_size);
  }

  #[test]
  fn counter_accessor_finds_figure() {
    let style = Style::default();
    assert_eq!(style.counter(CounterName::Figure).display_name, "Figure");
  }

  #[test]
  fn default_background_color_is_none() {
    let style = Style::default();
    assert!(style.background_color.is_none());
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
  fn theorem_accessor_finds_proof() {
    let style = Style::default();
    assert_eq!(style.theorem(TheoremClass::Proof).display_name, "Proof");
    assert!(style.theorem(TheoremClass::Proof).unnumbered);
  }

  #[test]
  fn validate_detects_invalid_theorem_top_margin() {
    let mut style = Style::default();
    style.theorems.theorem.style.top_margin = Length::pt(-0.1);
    assert!(style.theorems.theorem.validate().is_err());
  }

  #[test]
  fn background_color_field_accepts_color_value() {
    let style = Style {
      background_color: Some(Color::new(204, 179, 153)),
      ..Default::default()
    };
    assert!(style.validate().is_ok());
    let color = style.background_color.expect("background_color should be Some");
    assert_eq!(color.rgb(), [204, 179, 153]);
  }

  #[test]
  fn rejects_renamed_top_level_text_keys() {
    assert!(toml::from_str::<Style>("font_size = \"12pt\"\n").is_err());
    assert!(toml::from_str::<Style>("line_height_factor = 1.2\n").is_err());
  }
}
