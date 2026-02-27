//! TOML 設定ファイルのパース・検証・変換モジュール
//!
//! PDF 生成に必要な設定情報を TOML ファイルから読み込み、
//! ファイルパスの解決、バリデーション、型変換を実行して
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
//! パス解決（canonicalize）
//!   ↓
//! バリデーション実行
//!   ├→ ページサイズ、フォントサイズの妥当性
//!   ├→ 余白値の妥当性
//!   ├→ 背景色の範囲（0.0-1.0）
//!   ├→ フォント名の重複チェック
//!   ├→ バリアブルフォント軸の長さ
//!   └→ スクリプト・言語・フィーチャーコードの長さ
//!   ↓
//! 出力ディレクトリの作成
//!   ↓
//! Config（構造化設定）
//! ```
//!
//! ## バリデーション項目
//!
//! | 項目 | 条件 | エラー型 |
//! |-----|------|--------|
//! | page height/width | > 0 | `NonPositive` |
//! | `font_size` | > 0 | `NonPositive` |
//! | `line_height_factor` | > 0 | `NonPositive` |
//! | 各余白 | >= 0 | `NegativeMargin` |
//! | 余白合計（縦） | < height | `MarginSumTooLarge` |
//! | 余白合計（横） | < width | `MarginSumTooLarge` |
//! | 背景色 RGB | [0.0, 1.0] | `InvalidBackgroundColor` |
//! | 背景色 RGB 指定 | 全成分または全省略 | `PartialBackgroundColor` |
//! | `font_name` | 重複なし | `DuplicateFontName` |
//! | 軸名 | 4 文字 | `InvalidFontVariationAxisName` |
//! | script code | 4 文字 | `InvalidScriptCodeLength` |
//! | language code | 3 or 4 文字 | `InvalidLanguageCodeLength` |
//! | feature tag | 4 文字 | `InvalidFontFeatureTagLength` |
//! | フォントパス | 存在・解決可能 | `FontPathResolution` |
//!
//! ## 19 フォント種別
//!
//! 設定は 19 フォント種別に対応：
//! - Latin: `serif` × 4 + `sans_serif` × 4 + `monospace` × 4 = 12
//! - Special: `math` = 1
//! - Japanese: `serif` 2 + `sans_serif` 2 + `monospace` 2 = 6
//!
//! 各フォント種別は独立した `FontConfig` を持ちます。
//!
//! ## 使用例
//!
//! ```ignore
//! # use read_config::*;
//!
//! // デフォルトパスから読み込み
//! let config = read_config()?;
//!
//! // PDF サイズ、余白、フォント設定にアクセス
//! println!("PDF size: {} x {}", config.pdf.width, config.pdf.height);
//! println!("Font: {}", config.font_configs.serif.font_name);
//! ```

// thiserror の #[error] マクロ生成コードへの誤検知を抑制（thiserror との既知の問題）。
#![allow(unused_assignments)]

use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
};

use miette::{Diagnostic, Report};
use thiserror::Error;
use tracing::info;
use types::FontType;

mod pre_config;
use pre_config::{PreConfig, PreFontConfig};
mod processed_config;

// processed_config の型を公開
pub use processed_config::{Config, Feature, FontConfig, FontConfigs, Margin, PdfConfig, VariationAxis};

const DEFAULT_CONFIG_PATH: &str = "./config/config.toml";

/// 設定ファイル読み込みで発生するすべてのエラー。
#[derive(Debug, Error, Diagnostic)]
enum ReadConfigError {
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
enum ValidationError {
  /// `height`/`width`/`font_size`/`line_height_factor` が正でない
  #[error("'{field}' は正の値である必要があります。")]
  #[diagnostic(code(config::validation::non_positive), help("0 より大きい値を指定してください。"))]
  NonPositive { field: &'static str },
  /// 余白が負の値
  #[error("'{field}' は 0 以上である必要があります。")]
  #[diagnostic(code(config::validation::negative_margin), help("負でない値を指定してください。"))]
  NegativeMargin { field: &'static str },
  /// 余白の合計がページ寸法以上
  #[error("方向 {axis} の余白合計 ({sum}) が寸法 {limit} 未満である必要があります。")]
  #[diagnostic(code(config::validation::margin_sum), help("余白の合計が寸法未満になるように調整してください。"))]
  MarginSumTooLarge {
    axis: &'static str,
    sum: f32,
    limit: f32,
  },
  /// `font_name` の重複
  #[error("フォント名 '{font_name}' が重複しています。")]
  #[diagnostic(
    code(config::validation::duplicate_font),
    help("各フォント種別に異なるフォント名を指定してください。")
  )]
  DuplicateFontName { font_name: String },
  /// 背景色が [0.0, 1.0] の範囲外
  #[error("背景色 {field} は [0.0, 1.0] の範囲である必要があります。指定値: {value}")]
  #[diagnostic(code(config::validation::background_color), help("0.0 から 1.0 の間の値を指定してください。"))]
  InvalidBackgroundColor { field: &'static str, value: f32 },
  /// バリアブルフォント軸名が 4 文字でない
  #[error("フォント軸名は 4 文字である必要があります。指定値: '{axis_name}'")]
  #[diagnostic(
    code(config::validation::font_axis_length),
    help("OpenType 軸タグ（例：'wght'、'wdth'）として 4 文字を指定してください。")
  )]
  InvalidFontVariationAxisName { axis_name: String },
  /// スクリプトコードが 4 文字でない
  #[error("スクリプトコードは 4 文字である必要があります。指定値: '{code}'")]
  #[diagnostic(
    code(config::validation::script_code),
    help("ISO 15924 スクリプトコード（例：'latn'、'arab'）として 4 文字を指定してください。")
  )]
  InvalidScriptCodeLength { code: String },
  /// 言語コードが 3 or 4 文字でない
  #[error("言語コードは 3 または 4 文字である必要があります。指定値: '{code}'")]
  #[diagnostic(
    code(config::validation::language_code),
    help("BCP 47 言語タグ（例：'eng'、'ja'）を指定してください。")
  )]
  InvalidLanguageCodeLength { code: String },
  /// フィーチャータグが 4 文字でない
  #[error("フィーチャータグは 4 文字である必要があります。指定値: '{tag}'")]
  #[diagnostic(
    code(config::validation::font_feature_tag),
    help("OpenType フィーチャータグ（例：'liga'、'smcp'）として 4 文字を指定してください。")
  )]
  InvalidFontFeatureTagLength { tag: String },
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
  /// 背景色の一部のみ指定（R/G/B が揃っていない）
  #[error("背景色は background_r、background_g、background_b の 3 成分をすべて指定する必要があります。")]
  #[diagnostic(
    code(config::validation::partial_background_color),
    help("3 成分をすべて指定するか、すべて省略してください。")
  )]
  PartialBackgroundColor,
}

/// `./config/config.toml` から設定を読み込みます。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション失敗時にエラーを返します。
pub fn read_config() -> miette::Result<Config> {
  let config_path = DEFAULT_CONFIG_PATH;
  info!(config_path = %config_path, "設定ファイルの読み込みを開始します");
  let config = read_config_with_path(config_path)?;
  info!(
    config_path = %config_path,
    document_name = %config.name,
    output_path = %config.pdf.output_path.display(),
    "設定ファイルの読み込みが完了しました"
  );
  return Ok(config);
}

/// 指定パスから設定ファイルを読み込みます。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション・出力パス構築の失敗時にエラーを返します。
fn read_config_with_path<P: AsRef<Path>>(config_path: P) -> miette::Result<Config> {
  let config_path = config_path.as_ref();
  let config_content = fs::read(config_path).map_err(|source| ReadConfigError::ReadFile {
    path: config_path.display().to_string(),
    source,
  })?;
  let pre_config: PreConfig = toml::from_slice(&config_content).map_err(|source| ReadConfigError::ParseToml {
    path: config_path.display().to_string(),
    source,
  })?;
  let current_dir = std::env::current_dir().map_err(|source| ReadConfigError::CurrentDir { source })?;
  let mut errors = Vec::new();

  // 構造体分解
  let pre_config::PreConfig {
    name,
    pdf: pre_pdf_config,
    font_configs: pre_font_configs,
  } = pre_config;
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
  } = pre_pdf_config;

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

  // 19 フォント種別を先にすべて変換しエラーを蓄積（途中で中断しない）
  let font_config_results = FontType::ALL
    .iter()
    .map(|font_type| to_font_config(*font_type, pre_font_configs.get(*font_type), &mut errors))
    .collect::<Vec<_>>();

  // フォント変換エラーを報告（重複チェック前）
  if !errors.is_empty() {
    return Err(Report::new(ReadConfigError::MultipleValidationErrors { errors }));
  }

  // errors が空なので全 19 種別が揃っていることが保証される
  #[allow(clippy::expect_used)]
  let font_configs =
    FontConfigs::from_all(font_config_results.into_iter().map(|r| r.expect("フォント設定の変換に失敗")));

  // font_name の重複チェック
  check_duplicate_font_names(&font_configs, &mut errors);

  // 重複エラーを報告
  if !errors.is_empty() {
    return Err(Report::new(ReadConfigError::MultipleValidationErrors { errors }));
  }

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
    font_configs,
  };

  return Ok(config);
}

/// 各フィールドが正（> 0）であることを検証します。
fn validate_positive_fields(fields: &[(&'static str, f32)], errors: &mut Vec<ValidationError>) {
  for (field, value) in fields {
    if *value <= 0.0 {
      errors.push(ValidationError::NonPositive { field });
    }
  }
}

/// 各余白が非負（>= 0）であることを検証します。
fn validate_non_negative_margins(fields: &[(&'static str, f32)], errors: &mut Vec<ValidationError>) {
  for (field, value) in fields {
    if *value < 0.0 {
      errors.push(ValidationError::NegativeMargin { field });
    }
  }
}

/// 上下の余白合計 < height、左右の余白合計 < width であることを検証します。
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

/// 出力ディレクトリを作成・正規化し、`{name}.pdf` の絶対パスを返します。
///
/// # Errors
///
/// ディレクトリ作成またはパス正規化に失敗した場合にエラーを返します。
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

/// R/G/B を検証（各成分 [0.0, 1.0]）し、3 成分揃っていれば RGB タプルを返します。
/// 一部のみ指定された場合は `errors` に記録します。3 成分すべて省略時は `None` を返します。
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
    (None, None, None) => return None,
    _ => {
      errors.push(ValidationError::PartialBackgroundColor);
      return None;
    },
  }
}

/// `PreFontConfig` を `FontConfig` に変換します。
///
/// パス解決に失敗した場合は `errors` にエラーを追加して `None` を返します。
/// これにより後続フォントのバリデーションを継続できます。
fn to_font_config(
  font_type: FontType,
  pre_font_config: &PreFontConfig,
  errors: &mut Vec<ValidationError>,
) -> Option<FontConfig> {
  let script = parse_script_code(pre_font_config.script.as_deref(), errors);
  let language = parse_language_code(pre_font_config.language.as_deref(), errors);
  let features = parse_font_features(pre_font_config.features.as_deref(), errors);
  let variation_axes = convert_axes(pre_font_config.variation_axes.as_deref(), errors);

  let font_path = match pre_font_config.font_path.canonicalize() {
    Ok(path) => path,
    Err(source) => {
      errors.push(ValidationError::FontPathResolution {
        font_type,
        path: pre_font_config.font_path.display().to_string(),
        source,
      });
      return None;
    },
  };

  return Some(FontConfig {
    font_name: pre_font_config.font_name.clone(),
    font_path,
    font_index: pre_font_config.font_index.unwrap_or(0),
    variation_axes,
    script,
    language,
    features,
  });
}

/// OpenType スクリプトコード（4 文字）を `[u8; 4]` に変換します。
/// 不正な長さの場合は `errors` に記録して `None` を返します。
fn parse_script_code(input: Option<&str>, errors: &mut Vec<ValidationError>) -> Option<[u8; 4]> {
  match input {
    Some(s) => {
      if s.len() == 4 {
        let bytes = s.as_bytes();
        let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
        return Some(arr);
      }
      errors.push(ValidationError::InvalidScriptCodeLength {
        code: s.to_string(),
      });
      return None;
    },
    None => return None,
  }
}

/// 言語コード（3 or 4 文字）を `[u8; 4]` に変換します。
/// 3 文字の場合は末尾にスペース（0x20）を補完します。
/// 不正な長さの場合は `errors` に記録して `None` を返します。
fn parse_language_code(input: Option<&str>, errors: &mut Vec<ValidationError>) -> Option<[u8; 4]> {
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
      errors.push(ValidationError::InvalidLanguageCodeLength {
        code: s.to_string(),
      });
      return None;
    },
    None => return None,
  }
}

/// OpenType フィーチャータグ（4 文字）を変換します。
/// 不正な長さのタグは `errors` に記録してスキップします。
fn parse_font_features(
  input: Option<&[pre_config::PreFontFeature]>,
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
          tag: pre_feature.tag.clone(),
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

/// バリアブルフォント軸を変換します。
///
/// 軸名が 4 文字でない場合は `errors` に記録しダミー値 `[0, 0, 0, 0]` を使用します。
fn convert_axes(
  axes: Option<&[pre_config::PreVariationAxis]>,
  errors: &mut Vec<ValidationError>,
) -> Option<Vec<VariationAxis>> {
  let axes = axes?;
  let result = axes
    .iter()
    .map(|axis| {
      if axis.name.len() == 4 {
        #[allow(clippy::unwrap_used)]
        let name = axis.name.as_bytes().try_into().unwrap();
        return VariationAxis {
          name,
          value: axis.value,
        };
      }
      errors.push(ValidationError::InvalidFontVariationAxisName {
        axis_name: axis.name.clone(),
      });
      return VariationAxis {
        name: [0, 0, 0, 0],
        value: axis.value,
      };
    })
    .collect::<Vec<_>>();
  return Some(result);
}

/// 19 フォント種別の `font_name` 重複を検出します。
fn check_duplicate_font_names(fonts: &FontConfigs, errors: &mut Vec<ValidationError>) {
  let mut set = HashSet::new();
  for (_, config) in fonts {
    if !set.insert(&config.font_name) {
      errors.push(ValidationError::DuplicateFontName {
        font_name: config.font_name.clone(),
      });
    }
  }
}
