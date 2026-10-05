//! `style.toml`（見た目）のデータモデル・既定値・読込・検証
//!
//! 物理・実体・メタデータ（`config.toml`）を所有する [`crate::project::config`] とは互いを知らず、
//! どちらか一方だけでは判定できない横断制約は [`crate::typeset::PreparedGeometry::prepare`] が持つ。
//! CSL ファイル自体は読まず、`csl_path` / `locale_path` の解決と存在確認までで止める。

mod block_alignment;
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
mod quote;
mod reference;
mod running;
mod table;
mod template;
mod text;
mod theorem;
mod title_page;
mod toc;

use garde::Validate;
use serde::Deserialize;
use tracing::debug;

pub(crate) use crate::style::error::ReadStyleError;
#[cfg_attr(
  not(test),
  expect(
    unused_imports,
    reason = "`NestedOrderedFormat` / `NumberStyle` は利用側が `#[cfg(test)] mod tests` だけで、本体ビルドでは未使用に見える"
  )
)]
pub(crate) use crate::style::{
  block_alignment::BlockAlignment,
  caption::CaptionStyle,
  counter::{CounterName, CounterStyles},
  footnote::{FootnoteNumbering, FootnoteStyle},
  list::NestedOrderedFormat,
  math::{MathScriptStyle, NumberSide},
  number_style::NumberStyle,
  page_numbering::PageNumberingStyle,
  running::RunningContentStyle,
  template::{
    CounterPlaceholder, CounterTemplate, NumberTemplate, NumberTitleTemplate, RefTemplate, RunningTemplate,
    RunningValues, TheoremHeadingTemplate, TheoremHeadingValues,
  },
  theorem::{TheoremReset, TheoremStyle},
  title_page::TitlePageStyle,
  toc::TocStyle,
};
use crate::{
  color::Color,
  failures::Failures,
  project::{self, InFile, PathResolver, ProjectPath, ProjectSource, TomlErrorParts},
  style::{
    columns::ColumnsStyle, error::StyleValidationError, figure::FigureStyle, heading::HeadingStyles,
    hyperref::HyperrefStyle, index::IndexStyle, list::ListStyle, math::MathStyle, page::PageStyle, quote::QuoteStyle,
    reference::ReferenceStyle, table::TableStyle, text::TextBlockStyle, theorem::TheoremStyles,
  },
};

/// スタイル設定全体。`style.toml` をパースして得られるトップレベルの構造体。
#[derive(Debug, Clone, Default, Deserialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Style {
  /// 背景色。`None` は背景描画なし
  #[garde(skip)]
  pub background_color: Option<Color>,
  /// 見出し全 6 レベルのスタイル
  #[garde(dive)]
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
  pub counters: CounterStyles,
  /// 定理クラス定義テーブル（`[theorems.<class>]`、固定 10 種）
  #[garde(dive)]
  pub theorems: TheoremStyles,
  /// ページ番号のスタイル（前付け＝ローマ数字 / 本文＝算用数字）
  #[garde(dive)]
  pub page_numbering: PageNumberingStyle,
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

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はファイルを読み込まずに [`Style::default`] を返します。`path` は
/// `project::config::load` が解決済みの値を渡すので、ここでは再解決しません。`resolver` は
/// style.toml が持つ `csl_path` / `locale_path` の解決に使います。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・値検証・参照ファイルのパス解決に失敗した場合はエラーを返します。
pub(crate) fn load(
  source: &dyn ProjectSource,
  path: Option<&ProjectPath>,
  resolver: &PathResolver,
) -> Result<Style, Failures<ReadStyleError>> {
  let Some(path) = path else {
    debug!("スタイル設定ファイル未指定のため既定のスタイルを使用");
    return Ok(Style::default());
  };
  let path_str = path.to_string();

  let content = source.read_text(path).map_err(|source| {
    return Failures::single(ReadStyleError::ReadFile {
      path: path_str.clone(),
      source,
    });
  })?;

  let mut style = parse(&content, &path_str)?;

  if let Some(failures) =
    validation_failures(&path_str, resolve_reference_paths(&mut style.reference, source, resolver))
  {
    return Err(failures);
  }

  debug!(
    style_path = %path_str,
    font_size_pt = %style.text.font_size.to_pt(),
    line_height_factor = %style.text.line_height_factor,
    "スタイル設定ファイルを読込"
  );
  return Ok(style);
}

/// TOML 文字列を [`Style`] にパースし、値検証とロケールコードの正規化まで実行します（I/O なし）。
///
/// # Errors
///
/// TOML 解析または値検証に失敗した場合はエラーを返します。
pub(crate) fn parse(content: &str, source_path: &str) -> Result<Style, Failures<ReadStyleError>> {
  let mut style: Style =
    project::parse_toml(source_path, content).map_err(|TomlErrorParts { src, span, source }| {
      return Failures::single(ReadStyleError::ParseToml { src, span, source });
    })?;
  if let Err(errors) = validate_values(&style)
    && let Some(failures) = validation_failures(source_path, errors)
  {
    return Err(failures);
  }
  style.reference.normalize();
  return Ok(style);
}

/// 値検証の違反列を、1 件ずつ独立した leaf 診断として運ぶ非空集合へ変換する（空なら `None`）。
///
/// 各違反には、それを見つけたスタイルファイルのパス `path` を添える。
fn validation_failures(path: &str, errors: Vec<StyleValidationError>) -> Option<Failures<ReadStyleError>> {
  return Failures::from_vec(
    errors.into_iter().map(|error| return ReadStyleError::from(InFile::new(path, error))).collect(),
  );
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// 違反の並びは garde の走査順（`Style` のフィールド宣言順）。
fn validate_values(style: &Style) -> Result<(), Vec<StyleValidationError>> {
  let Err(report) = style.validate() else {
    return Ok(());
  };
  return Err(
    report
      .iter()
      .map(|(path, error)| {
        return StyleValidationError::Field {
          path: path.to_string(),
          message: error.to_string(),
        };
      })
      .collect(),
  );
}

/// `style.reference` の CSL 関連パス（`csl_path` / `locale_path`）を `resolver` で解決し、
/// `source.exists` でファイルの存在を同時に検証します（I/O フェーズ）。
fn resolve_reference_paths(
  reference: &mut ReferenceStyle,
  source: &dyn ProjectSource,
  resolver: &PathResolver,
) -> Vec<StyleValidationError> {
  let mut errors: Vec<StyleValidationError> = Vec::new();

  if let Some(path) = reference.csl_path.take() {
    let resolved = resolver.resolve(&path);
    if source.exists(&resolved) {
      reference.csl_path = Some(resolved);
    } else {
      errors.push(StyleValidationError::CslFileNotFound {
        path: resolved.to_string(),
      });
    }
  }

  if let Some(path) = reference.locale_path.take() {
    let resolved = resolver.resolve(&path);
    if source.exists(&resolved) {
      reference.locale_path = Some(resolved);
    } else {
      errors.push(StyleValidationError::LocaleFileNotFound {
        path: resolved.to_string(),
      });
    }
  }

  return errors;
}

#[cfg(test)]
mod tests {
  use std::{collections::BTreeSet, path::Path};

  use garde::Validate;
  use miette::Diagnostic;

  use crate::{
    length::Length,
    project::{MemoryProjectSource, PathResolver, ProjectPath},
    style::{ReferenceStyle, Style, load, resolve_reference_paths},
  };

  #[test]
  fn resolve_reference_paths_resolves_relative_csl_path_against_base_dir() {
    let source = MemoryProjectSource::new().with_text("/project/styles/ieee.csl", "");
    let resolver = PathResolver::new(Path::new("/project"));
    let mut reference = ReferenceStyle {
      csl_path: Some(ProjectPath::new("styles/./ieee.csl")),
      ..ReferenceStyle::default()
    };

    let errors = resolve_reference_paths(&mut reference, &source, &resolver);

    assert!(errors.is_empty(), "登録済みパスはエラーにならないはず: {errors:?}");
    assert_eq!(reference.csl_path, Some(ProjectPath::new("/project/styles/ieee.csl")));
  }

  #[test]
  fn load_reads_through_project_source() {
    let source = MemoryProjectSource::new().with_text("/project/style.toml", "");
    let path = ProjectPath::new("/project/style.toml");

    let style =
      load(&source, Some(&path), &PathResolver::new(Path::new("/project"))).expect("空の TOML は既定値になるはず");

    assert_eq!(style.text.font_size, Style::default().text.font_size);
  }

  #[test]
  fn load_attributes_a_missing_csl_path_to_the_style_file_it_read() {
    let toml = "[reference]\ncsl_path = \"missing.csl\"\n";
    let source = MemoryProjectSource::new().with_text("/project/themes/custom.toml", toml);
    let path = ProjectPath::new("/project/themes/custom.toml");

    let Err(failures) = load(&source, Some(&path), &PathResolver::new(Path::new("/project"))) else {
      panic!("CSL パスの違反を期待");
    };

    let message = failures.first().to_string();
    assert!(
      message.starts_with("/project/themes/custom.toml: CSL スタイルファイルが見つかりません"),
      "{message}"
    );
  }

  #[test]
  fn load_aggregates_both_missing_csl_and_locale_paths() {
    let toml = "[reference]\ncsl_path = \"missing.csl\"\nlocale_path = \"missing.xml\"\n";
    let source = MemoryProjectSource::new().with_text("/project/style.toml", toml);
    let path = ProjectPath::new("/project/style.toml");

    let result = load(&source, Some(&path), &PathResolver::new(Path::new("/project")));

    let Err(failures) = result else {
      panic!("2 件の検証エラーを期待");
    };
    let codes: BTreeSet<String> = failures
      .iter()
      .map(|error| return error.code().expect("leaf の code を持つはず").to_string())
      .collect();
    assert_eq!(
      codes,
      BTreeSet::from([
        "style::validation::csl_file_not_found".to_owned(),
        "style::validation::locale_file_not_found".to_owned()
      ])
    );
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
  fn rejects_renamed_top_level_text_keys() {
    assert!(toml::from_str::<Style>("font_size = \"12pt\"\n").is_err());
    assert!(toml::from_str::<Style>("line_height_factor = 1.2\n").is_err());
  }

  #[test]
  fn rejects_keys_renamed_to_typeface() {
    let cases = [
      ("text", "font_kind"),
      ("heading.section", "font_kind"),
      ("list", "marker_font_kind"),
      ("quote", "font_kind"),
      ("table", "head_font_kind"),
      ("table.caption", "font_kind"),
      ("figure.caption", "font_kind"),
      ("theorems.theorem.presentation", "font_kind"),
      ("theorems.theorem.presentation", "heading_font_kind"),
      ("header", "font_kind"),
      ("footer", "font_kind"),
      ("title_page", "title_font_kind"),
      ("title_page", "author_font_kind"),
      ("title_page", "date_font_kind"),
    ];
    for (table, key) in cases {
      let toml = format!("[{table}]\n{key} = \"serif\"\n");
      let message = toml::from_str::<Style>(&toml).unwrap_err().to_string();
      assert!(message.contains(&format!("unknown field `{key}`")), "[{table}].{key}: {message}");
    }
  }
}

/// TOML パース系のテスト。
#[cfg(test)]
mod parse_tests {
  use super::{ReadStyleError, Style, StyleValidationError, load, parse};
  use crate::{
    color::Color,
    document::{HeadingLevel, Typeface},
    length::Length,
    project::{FilesystemProjectSource, PathResolver, ProjectPath},
  };

  fn dummy_source() -> &'static str { return "test.toml"; }

  #[test]
  fn load_returns_default_when_path_is_none() {
    let source = FilesystemProjectSource;
    let style = load(&source, None, &PathResolver::new(std::path::Path::new("."))).unwrap();
    let default = Style::default();
    assert!((style.text.font_size.to_pt() - default.text.font_size.to_pt()).abs() < f32::EPSILON);
    assert!((style.text.line_height_factor - default.text.line_height_factor).abs() < f32::EPSILON);
    assert!(style.background_color.is_none());
    assert_eq!(style.heading[HeadingLevel::Part].format, default.heading[HeadingLevel::Part].format);
  }

  #[test]
  fn parse_overrides_only_specified_fields() {
    let toml = "[text]\nfont_size = \"15pt\"\n";
    let style = parse(toml, dummy_source()).unwrap();
    assert!((style.text.font_size.to_pt() - 15.0).abs() < f32::EPSILON);
    let default = Style::default();
    assert!((style.text.line_height_factor - default.text.line_height_factor).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_overrides_columns() {
    let toml = "[columns]\ncount = 2\ngap = \"24pt\"\n";
    let style = parse(toml, dummy_source()).unwrap();
    assert_eq!(style.columns.count, 2);
    assert!((style.columns.gap.to_pt() - 24.0).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_enables_flush_bottom() {
    let toml = "[page]\nflush_bottom = true\n";
    let style = parse(toml, dummy_source()).unwrap();
    assert!(style.page.flush_bottom);
  }

  #[test]
  fn parse_overrides_page_margins_individually() {
    let toml = "[page]\nmargin_left = \"30mm\"\n";
    let style = parse(toml, dummy_source()).unwrap();
    assert!((style.page.margin_left.to_pt() - Length::mm(30.0).to_pt()).abs() < f32::EPSILON);
    assert!((style.page.margin_right.to_pt() - 85.0).abs() < f32::EPSILON);
    assert!((style.page.margin_top.to_pt() - 99.0).abs() < f32::EPSILON);
    assert!((style.page.margin_bottom.to_pt() - 99.0).abs() < f32::EPSILON);
  }

  #[test]
  fn parse_fails_on_negative_page_margin() {
    let toml = "[page]\nmargin_top = \"-1pt\"\n";
    let failures = parse(toml, dummy_source()).unwrap_err();
    // 余白単体の不正は style の値検証（`style::validation::field`）が報告する
    let (first, rest) = failures.into_parts();
    assert!(rest.is_empty());
    assert!(matches!(
      first,
      ReadStyleError::Validation(ref failure)
        if matches!(failure.error(), StyleValidationError::Field { path, .. } if path == "page.margin_top")
    ));
  }

  #[test]
  fn parse_fails_on_unknown_page_key() {
    let toml = "[page]\nflush_botom = true\n";
    let result = parse(toml, dummy_source());
    assert!(result.is_err());
  }

  #[test]
  fn parse_fails_on_unknown_columns_key() {
    let toml = "[columns]\ncont = 2\n";
    let result = parse(toml, dummy_source());
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_overrides_header_and_footer() {
    let toml = concat!(
      "[header]\n",
      "right = \"{page} / {pages}\"\n",
      "font_size = \"9pt\"\n",
      "typeface = \"sans_serif\"\n",
      "rule_thickness = \"0.5pt\"\n",
      "rule_color = \"#333333\"\n",
      "[footer]\n",
      "center = \"{title}\"\n",
    );
    let style = parse(toml, dummy_source()).unwrap();
    assert_eq!(style.header.right.as_str(), "{page} / {pages}");
    assert!((style.header.font_size.to_pt() - 9.0).abs() < f32::EPSILON);
    assert_eq!(style.header.typeface, Typeface::SansSerif);
    assert!((style.header.rule_thickness.to_pt() - 0.5).abs() < f32::EPSILON);
    assert_eq!(style.header.rule_color.map(Color::rgb), Some([0x33, 0x33, 0x33]));
    assert_eq!(style.footer.center.as_str(), "{title}");
    assert_eq!(style.footer.left.as_str(), "");
    assert!(!style.header.is_blank());
  }

  #[test]
  fn parse_fails_on_unknown_header_key() {
    let toml = "[header]\nrght = \"{page}\"\n";
    let result = parse(toml, dummy_source());
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_unknown_nested_key() {
    let toml = "[heading.chapter]\nfont_sze = \"30pt\"\n";
    let result = parse(toml, dummy_source());
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_retired_ragged_right_alignment() {
    let toml = "[text]\nalignment = \"ragged_right\"\n";
    let result = parse(toml, dummy_source());
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_fails_on_justify_toc_and_index_alignment() {
    for toml in [
      "[toc]\nalignment = \"justify\"\n",
      "[index]\ntitle_alignment = \"justify\"\n",
    ] {
      let result = parse(toml, dummy_source());
      assert!(
        matches!(result.as_ref().map_err(|failures| return failures.first()), Err(ReadStyleError::ParseToml { .. })),
        "{toml}"
      );
    }
  }

  #[test]
  fn load_fails_on_nonexistent_path() {
    let path = std::path::PathBuf::from("/nonexistent/style.toml");
    let source = FilesystemProjectSource;
    let base_dir = path.parent().expect("フィクスチャパスは親ディレクトリを持つはず");

    let result = load(&source, Some(&ProjectPath::new(&path)), &PathResolver::new(base_dir));

    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadStyleError::ReadFile { .. })
    ));
  }
}

/// 値検証系のテスト。
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
        return validation.into_error();
      })
      .collect();
  }

  fn paths(errors: &[StyleValidationError]) -> Vec<&str> {
    return errors
      .iter()
      .map(|error| match error {
        StyleValidationError::Field { path, .. }
        | StyleValidationError::CslFileNotFound { path, .. }
        | StyleValidationError::LocaleFileNotFound { path, .. } => return path.as_str(),
      })
      .collect();
  }

  #[test]
  fn parse_rejects_toc_alignment_with_page_numbers() {
    // show_page_numbers は既定 true
    let toml = "[toc]\nalignment = \"center\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    assert_eq!(paths(&errors), vec!["toc.alignment"]);
    assert!(errors[0].to_string().contains("show_page_numbers = false"), "{errors:?}");
  }

  #[test]
  fn parse_accepts_center_toc_alignment_without_page_numbers() {
    // leader を書かない（既定はリーダー無し）ので、ページ番号なしの目次はリーダーについて何も書かずに通る
    let toml = "[toc]\nalignment = \"center\"\nshow_page_numbers = false\n";
    assert!(parse(toml, dummy_source()).is_ok());
  }

  #[test]
  fn parse_accepts_default_toc_alignment_with_page_numbers() {
    assert!(parse("[toc]\nenabled = true\n", dummy_source()).is_ok());
    assert!(parse("[toc]\nalignment = \"left\"\nshow_page_numbers = true\n", dummy_source()).is_ok());
  }

  #[test]
  fn parse_rejects_toc_leader_without_page_numbers() {
    let toml = "[toc]\nshow_page_numbers = false\nleader = \".\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    assert_eq!(paths(&errors), vec!["toc.leader"]);
    assert!(errors[0].to_string().contains("show_page_numbers = true"), "{errors:?}");
  }

  #[test]
  fn parse_rejects_empty_toc_leader() {
    let errors = expect_validation_errors(parse("[toc]\nleader = \"\"\n", dummy_source()));
    assert_eq!(paths(&errors), vec!["toc.leader"]);
  }

  #[test]
  fn parse_accepts_toc_leader_with_page_numbers() {
    assert!(parse("[toc]\nenabled = true\nleader = \".\"\n", dummy_source()).is_ok());
  }

  #[test]
  fn parse_reports_field_errors_and_toc_combination_together() {
    let toml = "[text]\nfont_size = \"0pt\"\n\n[toc]\nalignment = \"right\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    assert_eq!(paths(&errors), vec!["text.font_size", "toc.alignment"]);
  }

  #[test]
  fn parse_collects_multiple_validation_errors() {
    let toml = "[text]\nfont_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    let paths = paths(&errors);
    assert!(paths.contains(&"text.font_size"));
    assert!(paths.contains(&"heading.chapter.font_size"));
  }

  #[test]
  fn reports_nested_theorem_presentation_validation_error_with_path() {
    let toml = "[theorems.theorem.presentation]\ntop_margin = \"-1pt\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    let paths = paths(&errors);
    assert!(
      paths.contains(&"theorems.theorem.presentation.top_margin"),
      "expected theorems.theorem.presentation.top_margin in {paths:?}"
    );
  }

  #[test]
  fn reports_theorem_empty_display_name_with_path() {
    let toml = "[theorems.lemma]\ndisplay_name = \"\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    let paths = paths(&errors);
    assert!(paths.contains(&"theorems.lemma.display_name"), "expected theorems.lemma.display_name in {paths:?}");
  }

  #[test]
  fn reports_unknown_placeholders_across_fields_together() {
    let toml = "
[heading.section]
format = \"{nubmer} {title}\"

[footer]
center = \"{pagee}\"
";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    let paths = paths(&errors);
    assert!(paths.contains(&"heading.section.format"), "expected heading.section.format in {paths:?}");
    assert!(paths.contains(&"footer.center"), "expected footer.center in {paths:?}");
  }

  #[test]
  fn placeholder_error_message_names_the_offending_token() {
    let toml = "[math.block]\ntag_format = \"({num})\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
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
  fn reports_partial_counter_placeholder_error_with_path() {
    // 部分指定でも garde の dive が効く
    let toml = "[counters.section]\nnumber_format = \"{chaptr}.{n}\"\n";
    let errors = expect_validation_errors(parse(toml, dummy_source()));
    let paths = paths(&errors);
    assert!(
      paths.contains(&"counters.section.number_format"),
      "expected counters.section.number_format in {paths:?}"
    );
  }

  #[test]
  fn parse_attributes_validation_errors_to_the_style_file_it_read() {
    let Err(failures) = parse("[text]\nfont_size = \"0pt\"\n", "themes/custom-style.toml") else {
      panic!("値検証の違反を期待");
    };
    let message = failures.first().to_string();
    assert!(message.starts_with("themes/custom-style.toml: 'text.font_size': "), "{message}");
  }
}
