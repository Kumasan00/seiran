//! TOML 設定ファイルのパース・検証・変換モジュール
//!
//! PDF 生成に必要な設定情報を TOML ファイルから読み込み、
//! `garde` による宣言的バリデーション、ファイルパスの解決、型変換を実行して
//! アプリケーションが直接使用できる構造化設定データに変換します。
//!
//! ## 処理フロー
//!
//! ```text
//! config.toml（TOML テキスト）
//!   ↓
//! fs::read()（ファイル読み込み）
//!   ↓
//! toml::from_slice()（TOML パース → PreConfig）
//!   ↓
//! garde::Validate::validate（範囲・長さ・相互制約の検証）
//!   ↓
//! パス解決（canonicalize）
//!   ↓
//! 出力ディレクトリの作成
//!   ↓
//! Config（構造化設定）
//! ```
//!
//! ## 19 フォント種別
//!
//! 設定は 19 フォント種別に対応：
//! - Latin: `serif` × 4 + `sans_serif` × 4 + `monospace` × 4 = 12
//! - Special: `math` = 1
//! - Japanese: `serif` 2 + `sans_serif` 2 + `monospace` 2 = 6

use std::{
  fs,
  path::{Path, PathBuf},
};

use garde::Validate;
use miette::Diagnostic;
use thiserror::Error;
use tracing::info;
use types::FontType;

mod pre_config;
use pre_config::{PreConfig, PreFontConfig};
mod processed_config;

pub use processed_config::{Config, Feature, FontConfig, FontConfigs, Margin, PdfConfig, VariationAxis};

/// 設定ファイル読み込みで発生するすべてのエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum ReadConfigError {
  /// 設定ファイルの読み込み失敗
  #[error("設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// TOML 解析失敗
  #[error("設定ファイルの TOML 解析に失敗しました: {path}")]
  #[diagnostic(code(config::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    path: String,
    #[source]
    source: toml::de::Error,
  },
  /// 出力ディレクトリの作成失敗
  #[error("出力ディレクトリを作成できませんでした: {path}")]
  #[diagnostic(
    code(config::create_output_dir),
    help("親ディレクトリが存在し、書き込み権限があることを確認してください。")
  )]
  CreateOutputDir {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// 出力ディレクトリのパス正規化失敗
  #[error("出力ディレクトリのパスを正規化できませんでした: {path}")]
  #[diagnostic(code(config::canonicalize_output_dir), help("指定したディレクトリが存在するか確認してください。"))]
  CanonicalizeOutputDir {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// カレントディレクトリの取得失敗
  #[error("カレントディレクトリを取得できませんでした。")]
  #[diagnostic(code(config::current_dir), help("プロセスの作業ディレクトリが有効か確認してください。"))]
  CurrentDir {
    #[source]
    source: std::io::Error,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("複数のバリデーションエラーが発生しました。")]
  #[diagnostic(code(config::multiple_validation_errors))]
  MultipleValidationErrors {
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// 設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// garde が検出した設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(config::validation::field), help("config.toml の該当フィールドの値を確認してください。"))]
  Field { path: String, message: String },
  /// フォントパスを解決できない
  #[error("フォントファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::font_path),
    help("フォントファイルが存在し、読み取り権限があることを確認してください。")
  )]
  FontPathResolution {
    font_type: FontType,
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// スタイル設定ファイルのパスを解決できない
  #[error("スタイル設定ファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::style_path),
    help("スタイル設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  StylePathResolution {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// 参照設定ファイルのパスを解決できない
  #[error("参照設定ファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::references_path),
    help("参照設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  ReferencesPathResolution {
    path: String,
    #[source]
    source: std::io::Error,
  },
}

/// 指定パスから設定ファイルを読み込みます。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション・出力パス構築の失敗時にエラーを返します。
pub fn read_config(config_path: &PathBuf) -> Result<Config, ReadConfigError> {
  info!(config_path = %config_path.display(), "設定ファイルの読み込みを開始します");
  let config_content = fs::read(config_path).map_err(|source| ReadConfigError::ReadFile {
    path: config_path.display().to_string(),
    source,
  })?;
  let pre_config: PreConfig = toml::from_slice(&config_content).map_err(|source| ReadConfigError::ParseToml {
    path: config_path.display().to_string(),
    source,
  })?;

  // 検証フェーズ: garde の field-level 検証、相互制約、すべてのパス存在確認を
  // 単一の `errors` に集約して 1 度に報告する（I/O は副作用のないものに限る）。
  let mut errors: Vec<ValidationError> = Vec::new();
  if let Err(report) = pre_config.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }
  pre_config::validate_margin_sums(&pre_config.pdf, &mut errors);
  pre_config::validate_unique_font_names(&pre_config.font_configs, &mut errors);

  let style_path = canonicalize_or_record(pre_config.style_path.as_deref(), &mut errors, |path, source| {
    ValidationError::StylePathResolution { path, source }
  });
  let references_path =
    canonicalize_or_record(pre_config.references_path.as_deref(), &mut errors, |path, source| {
      ValidationError::ReferencesPathResolution { path, source }
    });

  // 19 フォント種別を変換しつつパス解決エラーも同じ `errors` に集約する
  let mut font_configs_vec: Vec<FontConfig> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    match to_font_config(font_type, pre_config.font_configs.get(font_type)) {
      Ok(font_config) => font_configs_vec.push(font_config),
      Err(error) => errors.push(error),
    }
  }

  if !errors.is_empty() {
    return Err(ReadConfigError::MultipleValidationErrors { errors });
  }

  // 副作用フェーズ: 検証通過後にディレクトリ作成や正規化を実行する
  let current_dir = std::env::current_dir().map_err(|source| ReadConfigError::CurrentDir { source })?;

  let pre_config::PreConfig {
    name,
    author,
    subject,
    style_path: _,
    references_path: _,
    pdf: pre_pdf_config,
    font_configs: _,
  } = pre_config;

  let output_path = build_output_pdf_path(&current_dir, &pre_pdf_config.output_dir, &name)?;
  let font_configs = FontConfigs::from_all(font_configs_vec);

  let config = Config {
    name,
    author,
    subject,
    style_path,
    references_path,
    pdf: PdfConfig {
      output_path,
      height: pre_pdf_config.height,
      width: pre_pdf_config.width,
      margin: Margin {
        top: pre_pdf_config.margin_top,
        bottom: pre_pdf_config.margin_bottom,
        left: pre_pdf_config.margin_left,
        right: pre_pdf_config.margin_right,
      },
    },
    font_configs,
  };

  info!(
    config_path = %config_path.display(),
    document_name = %config.name,
    output_path = %config.pdf.output_path.display(),
    "設定ファイルの読み込みが完了しました"
  );
  return Ok(config);
}

/// オプションのパスを `canonicalize` し、失敗時はエラーを `errors` に追加します。
///
/// `None` はそのまま `None` を返します。`Some(p)` の正規化に失敗した場合は
/// `make_err` で `ValidationError` を生成して `errors` に push し、戻り値は `None` とします。
/// 検証通過の判定は呼び出し側の `errors.is_empty()` で行うため、ここで早期 return しません。
fn canonicalize_or_record(
  path: Option<&Path>,
  errors: &mut Vec<ValidationError>,
  make_err: impl FnOnce(String, std::io::Error) -> ValidationError,
) -> Option<PathBuf> {
  match path {
    Some(p) => match p.canonicalize() {
      Ok(canon) => return Some(canon),
      Err(source) => {
        errors.push(make_err(p.display().to_string(), source));
        return None;
      },
    },
    None => return None,
  }
}

/// 出力ディレクトリを作成・正規化し、`{name}.pdf` の絶対パスを返します。
fn build_output_pdf_path(current_dir: &Path, output_dir: &Path, name: &str) -> Result<PathBuf, ReadConfigError> {
  let output_dir_path = if output_dir.is_absolute() {
    output_dir.to_path_buf()
  } else {
    current_dir.join(output_dir)
  };
  fs::create_dir_all(&output_dir_path).map_err(|source| ReadConfigError::CreateOutputDir {
    path: output_dir_path.display().to_string(),
    source,
  })?;
  let mut output_path = output_dir_path.canonicalize().map_err(|source| ReadConfigError::CanonicalizeOutputDir {
    path: output_dir_path.display().to_string(),
    source,
  })?;
  output_path.push(name);
  output_path.set_extension("pdf");
  return Ok(output_path);
}

/// `PreFontConfig` を `FontConfig` に変換します。
///
/// バイト長と ASCII はカスタムバリデーターで検証済みのため、`[u8; 4]` 変換は安全に実行できます。
/// パス解決に失敗した場合は `ValidationError::FontPathResolution` を返します。
fn to_font_config(font_type: FontType, pre_font_config: &PreFontConfig) -> Result<FontConfig, ValidationError> {
  let font_path = pre_font_config.font_path.canonicalize().map_err(|source| ValidationError::FontPathResolution {
    font_type,
    path: pre_font_config.font_path.display().to_string(),
    source,
  })?;

  let script = pre_font_config.script.as_deref().map(four_byte_tag);
  let language = pre_font_config.language.as_deref().map(language_tag);
  let features = pre_font_config.features.as_deref().and_then(|fs| {
    let v: Vec<Feature> = fs
      .iter()
      .map(|f| Feature {
        tag: four_byte_tag(&f.tag),
        value: f.value,
      })
      .collect();
    (!v.is_empty()).then_some(v)
  });
  let variation_axes = pre_font_config.variation_axes.as_deref().map(|axes| {
    axes
      .iter()
      .map(|a| VariationAxis {
        name: four_byte_tag(&a.name),
        value: a.value,
      })
      .collect::<Vec<_>>()
  });

  return Ok(FontConfig {
    font_name: pre_font_config.font_name.clone(),
    font_path,
    font_index: pre_font_config.font_index.unwrap_or(0),
    variation_axes,
    script,
    language,
    features,
  });
}

/// 4 バイト ASCII 文字列を `[u8; 4]` に変換します（カスタムバリデーターで長さと ASCII を検証済み）。
#[allow(clippy::unwrap_used)]
fn four_byte_tag(s: &str) -> [u8; 4] {
  return s.as_bytes().try_into().unwrap();
}

/// 3 or 4 文字の言語タグを `[u8; 4]` に変換します（3 文字時は末尾スペース）。
fn language_tag(s: &str) -> [u8; 4] {
  let b = s.as_bytes();
  if b.len() == 4 {
    return [b[0], b[1], b[2], b[3]];
  }
  return [b[0], b[1], b[2], b' '];
}

#[cfg(test)]
mod tests {
  use super::{Config, ReadConfigError, ValidationError, read_config};
  use std::fmt::Write as _;
  use std::path::PathBuf;
  use tempfile::TempDir;
  use types::FontType;

  /// 19 フォント種別すべての設定セクションを生成するヘルパー。
  ///
  /// 各セクションは一意の `font_name` を持ち、引数の `font_path` を共有します。
  fn make_font_sections(font_path: &str) -> String {
    const SECTION_NAMES: [&str; 19] = [
      "serif",
      "serif_bold",
      "serif_italic",
      "serif_bold_italic",
      "sans_serif",
      "sans_serif_bold",
      "sans_serif_italic",
      "sans_serif_bold_italic",
      "monospace",
      "monospace_bold",
      "monospace_italic",
      "monospace_bold_italic",
      "math",
      "japanese_serif",
      "japanese_serif_bold",
      "japanese_sans_serif",
      "japanese_sans_serif_bold",
      "japanese_monospace",
      "japanese_monospace_bold",
    ];
    let mut out = String::new();
    for name in SECTION_NAMES {
      write!(out, "[font_configs.{name}]\nfont_name = \"font_{name}\"\nfont_path = \"{font_path}\"\n\n").unwrap();
    }
    return out;
  }

  /// 既定の `[pdf]` セクション（妥当な値）を生成します。
  fn valid_pdf_section(output_dir: &str) -> String {
    return format!(
      "[pdf]\noutput_dir = \"{output_dir}\"\nheight = 842.0\nwidth = 595.0\n\
       margin_top = 50.0\nmargin_bottom = 50.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n"
    );
  }

  /// 一時ディレクトリにダミーのフォントファイルと `config.toml` を作成します。
  ///
  /// `build_toml` には `(font_path, output_dir)` が渡され、戻り値の TOML 文字列が
  /// `config.toml` として書き出されます。
  fn setup_config(build_toml: impl FnOnce(&str, &str) -> String) -> (TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().unwrap();
    let font_path = tempdir.path().join("dummy.ttf");
    std::fs::write(&font_path, b"").unwrap();
    let output_dir = tempdir.path().join("output");
    let config_path = tempdir.path().join("config.toml");
    let toml_text = build_toml(font_path.to_str().unwrap(), output_dir.to_str().unwrap());
    std::fs::write(&config_path, toml_text).unwrap();
    return (tempdir, config_path);
  }

  #[test]
  fn read_config_succeeds_with_valid_config() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir| {
      format!("name = \"test_doc\"\n\n{}{}", valid_pdf_section(output_dir), make_font_sections(font_path))
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert
    assert_eq!(config.name, "test_doc");
    assert_eq!(config.font_configs.iter().count(), 19);
    assert_eq!(config.font_configs.get(FontType::Serif).font_name, "font_serif");
  }

  #[test]
  fn read_config_fails_on_invalid_toml_syntax() {
    // Arrange
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("config.toml");
    std::fs::write(&config_path, "name = \nthis is not valid toml").unwrap();

    // Act
    let result = read_config(&config_path);

    // Assert
    assert!(matches!(result, Err(ReadConfigError::ParseToml { .. })));
  }

  #[test]
  fn read_config_fails_on_negative_margin() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir| {
      format!(
        "name = \"test\"\n\n[pdf]\noutput_dir = \"{output_dir}\"\nheight = 842.0\nwidth = 595.0\n\
         margin_top = -10.0\nmargin_bottom = 50.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n{}",
        make_font_sections(font_path),
      )
    });

    // Act
    let result = read_config(&config_path);

    // Assert
    assert!(matches!(result, Err(ReadConfigError::MultipleValidationErrors { .. })));
  }

  #[test]
  fn read_config_fails_on_margin_sum_exceeding_dimension() {
    // Arrange: vertical margin sum (60+60=120) >= height (100)
    let (_tempdir, config_path) = setup_config(|font_path, output_dir| {
      format!(
        "name = \"test\"\n\n[pdf]\noutput_dir = \"{output_dir}\"\nheight = 100.0\nwidth = 595.0\n\
         margin_top = 60.0\nmargin_bottom = 60.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n{}",
        make_font_sections(font_path),
      )
    });

    // Act
    let result = read_config(&config_path);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path == "pdf" && message.contains("vertical")
    )));
  }

  #[test]
  fn read_config_fails_on_duplicate_font_names_with_font_type_in_path() {
    // Arrange: serif_bold が serif と同じ font_name を使う
    let (_tempdir, config_path) = setup_config(|font_path, output_dir| {
      let sections = make_font_sections(font_path).replace(
        "[font_configs.serif_bold]\nfont_name = \"font_serif_bold\"",
        "[font_configs.serif_bold]\nfont_name = \"font_serif\"",
      );
      format!("name = \"test\"\n\n{}{sections}", valid_pdf_section(output_dir))
    });

    // Act
    let result = read_config(&config_path);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    let dup_path = errors
      .iter()
      .find_map(|error| match error {
        ValidationError::Field { path, message } if message.contains("重複") => Some(path.as_str()),
        _ => None,
      })
      .expect("expected duplicate font name error");
    assert!(dup_path.contains("SerifBold"), "path should contain FontType, got: {dup_path}");
  }

  #[test]
  fn read_config_fails_on_nonexistent_font_path() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|_font_path, output_dir| {
      format!(
        "name = \"test\"\n\n{}{}",
        valid_pdf_section(output_dir),
        make_font_sections("/nonexistent/path/to/font.ttf"),
      )
    });

    // Act
    let result = read_config(&config_path);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().all(|error| matches!(error, ValidationError::FontPathResolution { .. })));
    assert_eq!(errors.len(), 19);
  }
}
