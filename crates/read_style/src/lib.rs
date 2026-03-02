#![allow(unused_assignments)]

use std::path::Path;

use figment::{
  Figment,
  providers::{Format, Serialized, Toml},
};
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

const STYLE_PATH: &str = "config/style.toml";

/// スタイル設定ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum ReadStyleError {
  /// スタイル設定の読み込み・解析に失敗した場合
  #[error("スタイル設定ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(style::read), help("スタイル設定ファイルのパスと TOML の構文を確認してください。"))]
  ReadStyle {
    /// ファイルパス
    path: String,
    /// 元のエラー
    #[source]
    source: Box<figment::Error>,
  },
  /// `font_size` が 0 以下の場合
  #[error("スタイル設定ファイルの font_size は 0 より大きい必要があります:  font_size={font_size}")]
  #[diagnostic(code(style::font_size), help("style.toml の font_size に 0 より大きい値を設定してください。"))]
  InvalidFontSize {
    /// 不正なフォントサイズ
    font_size: f32,
  },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Style {
  pub font_size: f32,
  pub part: HeadingStyle,
  pub chapter: HeadingStyle,
  pub section: HeadingStyle,
  pub sub_section: HeadingStyle,
  pub paragraph: HeadingStyle,
  pub sub_paragraph: HeadingStyle,
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
      part: HeadingStyle::new(part, 40.0, 20.0, true, true),
      chapter: HeadingStyle::new(chapter, 25.0, 15.0, false, false),
      section: HeadingStyle::new(section, 20.0, 10.0, false, false),
      sub_section: HeadingStyle::new(sub_section, 16.0, 10.0, false, false),
      paragraph: HeadingStyle::new(paragraph, 14.0, 5.0, false, false),
      sub_paragraph: HeadingStyle::new(sub_paragraph, 12.0, 5.0, false, false),
    };
  }
}

/// 見出し要素のスタイル設定（フォントサイズと下余白）
#[derive(Debug, Deserialize, Serialize)]
pub struct HeadingStyle {
  pub format: String,
  pub font_size: f32,
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

/// Reads the style configuration from the style TOML file.
///
/// # Errors
///
/// Returns an error if the configuration file cannot be read or if the
/// configuration values cannot be extracted into a [`Style`] struct.
pub fn read_style<P: AsRef<Path>>(path: Option<P>) -> miette::Result<Style> {
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

  validate_style(&style)?;

  info!(font_size = style.font_size, "スタイル設定ファイルの読み込みが完了しました");
  return Ok(style);
}

fn validate_style(style: &Style) -> miette::Result<()> {
  if style.font_size <= 0.0 {
    return Err(
      ReadStyleError::InvalidFontSize {
        font_size: style.font_size,
      }
      .into(),
    );
  }
  return Ok(());
}
