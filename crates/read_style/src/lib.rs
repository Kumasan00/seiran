//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が `config/style.toml` を読み込み、figment によるデフォルト値マージと
//! `garde` による値検証を行って [`Style`] を返します。

use std::path::Path;

use figment2::{
  Figment,
  providers::{Format, Serialized, Toml},
};
use garde::Validate;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

const STYLE_PATH: &str = "config/style.toml";

/// スタイル設定ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ReadStyleError {
  /// スタイル設定の読み込み・解析に失敗した場合
  #[error("スタイル設定ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(style::read), help("スタイル設定ファイルのパスと TOML の構文を確認してください。"))]
  ReadStyle {
    /// ファイルパス
    path: String,
    /// 元のエラー
    #[source]
    source: Box<figment2::Error>,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("スタイル設定のバリデーションに失敗しました。")]
  #[diagnostic(code(style::multiple_validation_errors))]
  MultipleValidationErrors {
    /// 検証で検出されたすべてのエラー
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// スタイル設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// garde が検出したスタイル設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(style::validation::field), help("style.toml の該当フィールドの値を確認してください。"))]
  Field {
    /// 不正なフィールドのパス（例: `font_size`, `part.font_size`）
    path: String,
    /// 不正の内容
    message: String,
  },
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Style {
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 背景色 RGB（0.0-1.0、オプション）。未指定時は背景色なし。
  #[garde(custom(validate_background_color))]
  pub background_color: Option<[f32; 3]>,
  #[garde(dive)]
  pub part: HeadingStyle,
  #[garde(dive)]
  pub chapter: HeadingStyle,
  #[garde(dive)]
  pub section: HeadingStyle,
  #[garde(dive)]
  pub sub_section: HeadingStyle,
  #[garde(dive)]
  pub paragraph: HeadingStyle,
  #[garde(dive)]
  pub sub_paragraph: HeadingStyle,
  #[garde(dive)]
  pub reference: ReferenceStyle,
  // TODO(figure-equation-prep): figure / equation / table 用 *Style 構造体の追加予定地。
  // 実装本体タスクで [counters] テーブルおよび FigureStyle / EquationStyle /
  // TableStyle を追加し、`parser::evaluator::counter::CounterRegistry::from_style`
  // と組み合わせて lowering までカスタマイズできるようにする。
}

impl Default for Style {
  fn default() -> Self {
    let part = "第\\partnum部 \\text".to_string();
    let chapter = "第\\chapternum章 \\text".to_string();
    let section = "\\chapternum.\\sectionnum".to_string();
    let sub_section = "\\chapternum.\\sectionnum.\\subsectionnum".to_string();
    let paragraph = "\\chapternum.\\sectionnum.\\subsectionnum.\\paragraphnum".to_string();
    let sub_paragraph = "\\chapternum.\\sectionnum.\\subsectionnum.\\paragraphnum.\\subparagraphnum".to_string();
    return Self {
      font_size: 12.0,
      line_height_factor: 1.2,
      background_color: None,
      part: HeadingStyle::new(part, 40.0, 20.0, true, true),
      chapter: HeadingStyle::new(chapter, 25.0, 15.0, false, false),
      section: HeadingStyle::new(section, 20.0, 10.0, false, false),
      sub_section: HeadingStyle::new(sub_section, 16.0, 10.0, false, false),
      paragraph: HeadingStyle::new(paragraph, 14.0, 5.0, false, false),
      sub_paragraph: HeadingStyle::new(sub_paragraph, 12.0, 5.0, false, false),
      reference: ReferenceStyle::default(),
    };
  }
}

/// `background_color` の各成分が [0.0, 1.0] の範囲かを検証します。
///
/// `None` はそのまま通過させます。NaN や Infinity は範囲チェックで自動的に弾かれます。
/// 引数の型は `garde` のカスタムバリデーター API に従います。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_background_color(value: &Option<[f32; 3]>, _: &()) -> garde::Result {
  let Some([r, g, b]) = value else {
    return Ok(());
  };
  for (component, v) in [("R", *r), ("G", *g), ("B", *b)] {
    if !(0.0..=1.0).contains(&v) {
      return Err(garde::Error::new(format!(
        "background_color の {component} 成分は [0.0, 1.0] の範囲である必要があります: {v}"
      )));
    }
  }
  return Ok(());
}

/// 見出し要素のスタイル設定（フォントサイズと下余白）
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct HeadingStyle {
  pub format: String,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  pub page_break_before: bool,
  pub page_break_after: bool,
}

impl HeadingStyle {
  /// 新しい [`HeadingStyle`] を作成する
  #[must_use]
  const fn new(
    format: String,
    font_size: f32,
    bottom_margin: f32,
    page_break_before: bool,
    page_break_after: bool,
  ) -> Self {
    return Self {
      format,
      font_size,
      bottom_margin,
      page_break_before,
      page_break_after,
    };
  }
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct ReferenceStyle {
  pub format: String,
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      format: "参考文献".to_string(),
      font_size: 12.0,
      bottom_margin: 10.0,
    };
  }
}

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はデフォルト位置 `config/style.toml` を読み込みます。
/// ファイル読み込み後は [`validate_values`] による値検証も併せて実行します。
///
/// # Errors
///
/// ファイルが読めない、TOML 解析に失敗、値検証に違反した場合にエラーを返します。
pub fn read_style<P: AsRef<Path>>(path: Option<P>) -> Result<Style, ReadStyleError> {
  let figment = Figment::from(Serialized::defaults(Style::default()));
  let (figment, style_path_str) = if let Some(p) = path {
    let path_str = p.as_ref().display().to_string();
    info!(style_path = %path_str, "スタイル設定ファイルの読み込みを開始します");
    (figment.merge(Toml::file(p)), path_str)
  } else {
    info!(style_path = STYLE_PATH, "デフォルトのスタイル設定ファイルの読み込みを開始します");
    (figment.merge(Toml::file(STYLE_PATH)), STYLE_PATH.to_string())
  };
  let style: Style = figment.extract().map_err(|source| ReadStyleError::ReadStyle {
    path: style_path_str,
    source: Box::new(source),
  })?;

  if let Err(errors) = validate_values(&style) {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }

  info!(
    font_size = style.font_size,
    line_height_factor = style.line_height_factor,
    "スタイル設定ファイルの読み込みが完了しました"
  );
  return Ok(style);
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証および `background_color` のカスタム検証を集約します。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(style: &Style) -> Result<(), Vec<ValidationError>> {
  let Err(report) = style.validate() else {
    return Ok(());
  };
  let errors = report
    .iter()
    .map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    })
    .collect();
  return Err(errors);
}
