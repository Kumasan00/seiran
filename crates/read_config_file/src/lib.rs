//! 設定ファイル読み込みモジュール
//!
//! このモジュールは、TOML形式の設定ファイルを読み込み、
//! パスの解決、バリデーション、型変換を行って
//! アプリケーションが使用する設定情報に変換します。

#![allow(unused_assignments)]

use std::{
  fs, io,
  path::{Path, PathBuf},
};

use miette::{Diagnostic, Report};
use thiserror::Error;
use tracing::info;

mod pre_config;
use pre_config::PreConfig;
mod processed_config;

// processed_config の型を公開
pub use processed_config::{Config, Feature, FontConfig, FontConfigs, Margin, PdfConfig, VariationAxis};

/// 設定ファイル読み込みに関連するエラー
///
/// ファイルI/O、TOML解析、設定値のバリデーションに関する
/// 様々なエラーケースを表現します。
#[derive(Debug, Error, Diagnostic)]
enum ReadConfigError {
  /// 入出力エラー
  #[error("failed to read config file")]
  #[diagnostic(code(config::io), help("設定ファイルのパスと権限を確認してください"))]
  Io(#[from] io::Error),
  /// TOML解析エラー
  #[error("TOML parse error: {0}")]
  #[diagnostic(code(config::toml_parse), help("TOML の構文が正しいか確認してください"))]
  Toml(#[from] toml::de::Error),
  /// 出力ディレクトリの作成に失敗
  #[error("failed to create output directory")]
  #[diagnostic(code(config::create_dir), help("ディレクトリの権限またはパスを確認してください"))]
  CreateDir {
    #[source]
    source: io::Error,
  },
  /// カレントディレクトリの取得に失敗
  #[error("failed to get current working directory")]
  #[diagnostic(code(config::current_dir))]
  CurrentDir {
    #[source]
    source: io::Error,
  },
  /// パスの正規化に失敗
  #[error("failed to canonicalize path '{path}'")]
  #[diagnostic(code(config::canonicalize), help("存在するパスかどうか確認してください"))]
  Canonicalize {
    path: PathBuf,
    #[source]
    source: io::Error,
  },
  /// 複数のバリデーションエラー
  #[error("validation errors occurred")]
  #[diagnostic()]
  MultipleValidationErrors {
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// 設定値のバリデーションエラー詳細
/// 各種設定値に対する具体的なバリデーションエラーを表現します。
#[derive(Debug, Error, Diagnostic)]
enum ValidationError {
  /// サイズ系が正でない（`height`/`width`/`font_size`/`line_height_factor`）
  #[error("'{field}' must be > 0")]
  #[diagnostic(code(config::validation::non_positive), help("正の値を指定してください"))]
  NonPositive { field: &'static str },
  /// 余白が負の値
  #[error("'{field}' must be >= 0")]
  #[diagnostic(code(config::validation::negative_margin))]
  NegativeMargin { field: &'static str },
  /// 余白の合計が寸法以上
  #[error("margin sum for {axis} ({sum} ) must be < {limit}")]
  #[diagnostic(code(config::validation::margin_sum), help("余白の合計が寸法未満になるよう調整してください"))]
  MarginSumTooLarge {
    axis: &'static str,
    sum: f32,
    limit: f32,
  },
  /// `font_name`の重複
  #[error("Duplicate font_name found: '{font_name}'")]
  #[diagnostic(code(config::validation::duplicate_font))]
  DuplicateFontName { font_name: String },
  /// 背景色の範囲エラー
  #[error("Background color '{field}' must be in [0.0, 1.0], got {value}")]
  #[diagnostic(code(config::validation::background_color))]
  InvalidBackgroundColor { field: &'static str, value: f32 },
  /// バリアブルフォントタグの長さエラー
  #[error("Font variation axis name must be 4 characters long, got '{axis_name}'")]
  #[diagnostic(code(config::validation::font_axis_length))]
  InvalidFontVariationAxisName { axis_name: String },
  /// scriptコードの長さエラー
  #[error("Script code must be 3 or 4 characters long, got '{code}'")]
  #[diagnostic(code(config::validation::script_code))]
  InvalidScriptCodeLength { code: String },
  /// languageコードの長さエラー
  #[error("Language code must be 3 or 4 characters long, got '{code}'")]
  #[diagnostic(code(config::validation::language_code))]
  InvalidLanguageCodeLength { code: String },
  /// featureタグの長さエラー
  #[error("Font feature tag must be 4 characters long, got '{tag}'")]
  #[diagnostic(code(config::validation::font_feature_tag))]
  InvalidFontFeatureTagLength { tag: String },
}

/// デフォルトパスから設定ファイルを読み込み
///
/// `./config/config.toml`から設定を読み込み、パスの解決とバリデーションを行います。
///
/// # 戻り値
///
/// 解析され、検証済みの設定情報を返します。
///
/// # Errors
///
/// ファイルの読み込み、解析、またはバリデーションに失敗した場合にエラーを返します。
/// # エラー
///
/// ファイルの読み込み、解析、またはバリデーションに失敗した場合にエラーを返します。
/// エラーは `miette::Report` でラップされ、詳細なエラー情報が提供されます。
pub fn read_config_file() -> Result<Config, Report> {
  let config_path = "./config/config.toml";
  info!(config_path = %config_path, "Reading config file");
  return read_config_file_with_path(config_path).map_err(std::convert::Into::into);
}

/// 指定されたパスから設定ファイルを読み込み
///
/// TOMLファイルを読み込み、解析後に以下の処理を実行します：
/// - ファイルパスの解決と正規化
/// - ページサイズ、余白、フォントサイズのバリデーション
/// - 背景色の検証（0.0〜1.0の範囲）
/// - フォント名の重複チェック
/// - 出力ディレクトリの作成
///
/// # 引数
///
/// * `config_path` - 設定ファイルのパス
///
/// # 戻り値
///
/// 解析され、検証済みの設定情報を返します。
///
/// # エラー
///
/// ファイルの読み込み、解析、バリデーションのいずれかで失敗した場合にエラーを返します。
/// 内部的には `ReadConfigError` を使用しますが、公開APIでは `miette::Report` に変換されます。
fn read_config_file_with_path<P: AsRef<Path>>(config_path: P) -> Result<Config, ReadConfigError> {
  let config_content = fs::read(config_path)?;
  let pre_config: PreConfig = toml::from_slice(&config_content)?;
  let current_dir = std::env::current_dir().map_err(|error| ReadConfigError::CurrentDir { source: error })?;
  let mut errors = Vec::new();

  // 構造体分解
  let pre_config::PreConfig {
    name,
    pdf: pre_pdf,
    font_configs: pre_font_configs,
  } = pre_config;
  let pre_config::PreFontConfigs {
    serif,
    serif_bold,
    serif_italic,
    serif_bold_italic,
    sans_serif,
    sans_serif_bold,
    sans_serif_italic,
    sans_serif_bold_italic,
    monospace,
    monospace_bold,
    monospace_italic,
    monospace_bold_italic,
    math,
    japanese_serif,
    japanese_serif_bold,
    japanese_sans_serif,
    japanese_sans_serif_bold,
    japanese_monospace,
    japanese_monospace_bold,
  } = pre_font_configs;
  let pre_config::PrePdfConfig {
    output_dir,
    height,
    width,
    font_size,
    line_height_factor,
    margin_top,
    margin_bottom,
    margin_left,
    margin_right,
    background_r,
    background_g,
    background_b,
  } = pre_pdf;

  // バリデーション
  validate_positive_fields(
    &[
      ("pdf.height", height),
      ("pdf.width", width),
      ("pdf.font_size", font_size),
      ("pdf.line_height_factor", line_height_factor),
    ],
    &mut errors,
  );
  validate_non_negative_margins(
    &[
      ("pdf.margin_top", margin_top),
      ("pdf.margin_bottom", margin_bottom),
      ("pdf.margin_left", margin_left),
      ("pdf.margin_right", margin_right),
    ],
    &mut errors,
  );
  validate_margin_sums(height, width, margin_top, margin_bottom, margin_left, margin_right, &mut errors);

  // 出力ディレクトリ作成とパス解決
  let output_path = build_output_pdf_path(&current_dir, &output_dir, &name)?;

  // 背景色のバリデーションと生成
  let background_color = build_background_color(background_r, background_g, background_b, &mut errors);

  // FontConfig/MathFontConfig 生成
  let config = Config {
    name,
    pdf: PdfConfig {
      output_path,
      height,
      width,
      font_size,
      line_height_factor,
      margin: Margin {
        top: margin_top,
        bottom: margin_bottom,
        left: margin_left,
        right: margin_right,
      },
      background_color,
    },
    font_configs: FontConfigs {
      serif: to_font_config(serif, &mut errors)?,
      serif_bold: to_font_config(serif_bold, &mut errors)?,
      serif_italic: to_font_config(serif_italic, &mut errors)?,
      serif_bold_italic: to_font_config(serif_bold_italic, &mut errors)?,
      sans_serif: to_font_config(sans_serif, &mut errors)?,
      sans_serif_bold: to_font_config(sans_serif_bold, &mut errors)?,
      sans_serif_italic: to_font_config(sans_serif_italic, &mut errors)?,
      sans_serif_bold_italic: to_font_config(sans_serif_bold_italic, &mut errors)?,
      monospace: to_font_config(monospace, &mut errors)?,
      monospace_bold: to_font_config(monospace_bold, &mut errors)?,
      monospace_italic: to_font_config(monospace_italic, &mut errors)?,
      monospace_bold_italic: to_font_config(monospace_bold_italic, &mut errors)?,
      math: to_font_config(math, &mut errors)?,
      japanese_serif: to_font_config(japanese_serif, &mut errors)?,
      japanese_serif_bold: to_font_config(japanese_serif_bold, &mut errors)?,
      japanese_sans_serif: to_font_config(japanese_sans_serif, &mut errors)?,
      japanese_sans_serif_bold: to_font_config(japanese_sans_serif_bold, &mut errors)?,
      japanese_monospace: to_font_config(japanese_monospace, &mut errors)?,
      japanese_monospace_bold: to_font_config(japanese_monospace_bold, &mut errors)?,
    },
  };

  // font_nameの重複チェック
  check_duplicate_font_names(&config.font_configs, &mut errors);

  // エラーが蓄積されている場合は複合エラーとして返す
  if !errors.is_empty() {
    return Err(ReadConfigError::MultipleValidationErrors { errors });
  }

  return Ok(config);
}

fn to_font_config(
  pre_font_config: pre_config::PreFontConfig,
  errors: &mut Vec<ValidationError>,
) -> Result<FontConfig, ReadConfigError> {
  let script = parse_script_code(pre_font_config.script, errors);
  let language = parse_language_code(pre_font_config.language, errors);
  let features = parse_font_features(pre_font_config.features, errors);

  return Ok(FontConfig {
    font_name: pre_font_config.font_name,
    font_path: resolve_path_buf(pre_font_config.font_path)?,
    font_index: pre_font_config.font_index.unwrap_or(0),
    variation_axes: convert_axes(pre_font_config.variation_axes, errors)?,
    script,
    language,
    features,
  });
}

/// 相対パスを絶対パスに解決し正規化
///
/// `PathBuf`を受け取り、`canonicalize()`を使用してシンボリックリンクを解決し、
/// 正規化された絶対パスを返します。この関数はファイルが存在することを前提とします。
///
/// # 引数
///
/// * `path` - 解決するパス（所有権を移動）
///
/// # 戻り値
///
/// 正規化された絶対パスを返します。
///
/// # エラー
///
/// パスの正規化に失敗した場合、またはファイルが存在しない場合にエラーを返します。
fn resolve_path_buf(path: PathBuf) -> Result<PathBuf, ReadConfigError> {
  let resolved = path.canonicalize().map_err(|error| ReadConfigError::Canonicalize {
    path,
    source: error,
  })?;
  return Ok(resolved);
}

// variation_axes の変換
fn convert_axes(
  axes: Option<Vec<pre_config::PreVariationAxis>>,
  errors: &mut Vec<ValidationError>,
) -> Result<Option<Vec<VariationAxis>>, ReadConfigError> {
  axes
    .map(|axes| {
      axes
        .into_iter()
        .map(|axis| {
          if axis.name.len() == 4 {
            Ok(VariationAxis {
              #[allow(clippy::unwrap_used)]
              name: axis.name.as_bytes().try_into().unwrap(),
              value: axis.value,
            })
          } else {
            errors.push(ValidationError::InvalidFontVariationAxisName {
              axis_name: axis.name,
            });
            Ok(VariationAxis {
              name: [0, 0, 0, 0],
              value: axis.value,
            })
          }
        })
        .collect::<Result<Vec<_>, _>>()
    })
    .transpose()
}

/// scriptコード（4文字）を [u8;4] へ変換
fn parse_script_code(input: Option<String>, errors: &mut Vec<ValidationError>) -> Option<[u8; 4]> {
  match input {
    Some(s) => {
      if s.len() == 4 {
        let bytes = s.as_bytes();
        let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
        return Some(arr);
      }
      errors.push(ValidationError::InvalidScriptCodeLength { code: s });
      return None;
    },
    None => return None,
  }
}

/// languageコード（3または4文字）を [u8;4] へ変換（3文字の場合は末尾スペース）
fn parse_language_code(input: Option<String>, errors: &mut Vec<ValidationError>) -> Option<[u8; 4]> {
  match input {
    Some(s) => {
      if s.len() == 4 {
        let bytes = s.as_bytes();
        let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
        return Some(arr);
      } else if s.len() == 3 {
        let bytes = s.as_bytes();
        let arr = [bytes[0], bytes[1], bytes[2], b' '];
        return Some(arr);
      }
      errors.push(ValidationError::InvalidLanguageCodeLength { code: s.clone() });
      return None;
    },
    None => return None,
  }
}

fn parse_font_features(
  input: Option<Vec<pre_config::PreFontFeature>>,
  errors: &mut Vec<ValidationError>,
) -> Option<Vec<Feature>> {
  let mut features = Vec::new();
  if let Some(pre_features) = input {
    for pre_feature in pre_features {
      if pre_feature.tag.len() == 4 {
        #[allow(clippy::unwrap_used)]
        let tag_bytes: [u8; 4] = pre_feature.tag.as_bytes().try_into().unwrap();
        features.push(Feature {
          tag: tag_bytes,
          value: pre_feature.value,
        });
      } else {
        errors.push(ValidationError::InvalidFontFeatureTagLength {
          tag: pre_feature.tag,
        });
      }
    }
  }
  if features.is_empty() {
    None
  } else {
    Some(features)
  }
}
/// 指定された数値フィールドがすべて正であることを検証する
///
/// # 引数
/// * `fields` - (フィールド名, 値)の配列
///
/// # 戻り値
/// 成功時は`()`を返します。
fn validate_positive_fields(fields: &[(&'static str, f32)], errors: &mut Vec<ValidationError>) {
  for (field, value) in fields {
    if *value <= 0.0 {
      errors.push(ValidationError::NonPositive { field });
    }
  }
}

/// 余白フィールドがすべて非負であることを検証する
fn validate_non_negative_margins(fields: &[(&'static str, f32)], errors: &mut Vec<ValidationError>) {
  for (field, value) in fields {
    if *value < 0.0 {
      errors.push(ValidationError::NegativeMargin { field });
    }
  }
}

/// 余白の合計がページサイズ未満であることを検証する
fn validate_margin_sums(
  height: f32,
  width: f32,
  margin_top: f32,
  margin_bottom: f32,
  margin_left: f32,
  margin_right: f32,
  errors: &mut Vec<ValidationError>,
) {
  if margin_top + margin_bottom >= height {
    errors.push(ValidationError::MarginSumTooLarge {
      axis: "vertical",
      sum: margin_top + margin_bottom,
      limit: height,
    });
  }
  if margin_left + margin_right >= width {
    errors.push(ValidationError::MarginSumTooLarge {
      axis: "horizontal",
      sum: margin_left + margin_right,
      limit: width,
    });
  }
}

/// 出力PDFの絶対パスを生成する（ディレクトリ作成・正規化込み）
fn build_output_pdf_path(current_dir: &Path, output_dir: &Path, name: &str) -> Result<PathBuf, ReadConfigError> {
  let output_dir_path = if output_dir.is_absolute() {
    output_dir.to_path_buf()
  } else {
    current_dir.join(output_dir)
  };
  fs::create_dir_all(&output_dir_path).map_err(|error| ReadConfigError::CreateDir { source: error })?;
  let mut output_path = output_dir_path.canonicalize().map_err(|error| ReadConfigError::Canonicalize {
    path: output_dir_path.clone(),
    source: error,
  })?;
  output_path.push(name);
  output_path.set_extension("pdf");
  return Ok(output_path);
}

/// 背景色を検証し、指定があればRGBタプルを返す
fn build_background_color(
  r: Option<f32>,
  g: Option<f32>,
  b: Option<f32>,
  errors: &mut Vec<ValidationError>,
) -> Option<(f32, f32, f32)> {
  match (r, g, b) {
    (Some(r), Some(g), Some(b)) => {
      for (field, value) in [
        ("background_r", r),
        ("background_g", g),
        ("background_b", b),
      ] {
        if !(0.0..=1.0).contains(&value) {
          errors.push(ValidationError::InvalidBackgroundColor { field, value });
        }
      }
      return Some((r, g, b));
    },
    _ => return None,
  }
}

/// `font_name` の重複を検出
fn check_duplicate_font_names(fonts: &FontConfigs, errors: &mut Vec<ValidationError>) {
  let mut set = std::collections::HashSet::new();
  for name in [
    &fonts.serif.font_name,
    &fonts.serif_bold.font_name,
    &fonts.serif_italic.font_name,
    &fonts.serif_bold_italic.font_name,
    &fonts.sans_serif.font_name,
    &fonts.sans_serif_bold.font_name,
    &fonts.sans_serif_italic.font_name,
    &fonts.sans_serif_bold_italic.font_name,
    &fonts.monospace.font_name,
    &fonts.monospace_bold.font_name,
    &fonts.monospace_italic.font_name,
    &fonts.monospace_bold_italic.font_name,
    &fonts.math.font_name,
    &fonts.japanese_serif.font_name,
    &fonts.japanese_serif_bold.font_name,
    &fonts.japanese_sans_serif.font_name,
    &fonts.japanese_sans_serif_bold.font_name,
    &fonts.japanese_monospace.font_name,
    &fonts.japanese_monospace_bold.font_name,
  ] {
    if !set.insert(name) {
      errors.push(ValidationError::DuplicateFontName {
        font_name: name.clone(),
      });
    }
  }
}
