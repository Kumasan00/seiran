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
//! | `font_name` | 重複なし | `DuplicateFontName` |
//! | 軸名 | 4 文字 | `InvalidFontVariationAxisName` |
//! | script code | 4 文字 | `InvalidScriptCodeLength` |
//! | language code | 3 or 4 文字 | `InvalidLanguageCodeLength` |
//! | feature tag | 4 文字 | `InvalidFontFeatureTagLength` |
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
//! # use read_config_file::*;
//!
//! // デフォルトパスから読み込み
//! let config = read_config_file()?;
//!
//! // PDF サイズ、余白、フォント設定にアクセス
//! println!("PDF size: {} x {}", config.pdf.width, config.pdf.height);
//! println!("Font: {}", config.font_configs.serif.font_name);
//! ```

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

const DEFAULT_CONFIG_PATH: &str = "./config/config.toml";

/// 設定ファイル読み込みに関連するエラー
///
/// ファイルI/O、TOML解析、設定値のバリデーションに関する
/// 様々なエラーケースを表現します。
#[derive(Debug, Error, Diagnostic)]
enum ReadConfigError {
  /// ファイル読み込みエラー
  #[error("設定ファイルの読み込みに失敗しました。")]
  #[diagnostic(code(config::io), help("ファイルパスと読み込み権限を確認してください。"))]
  Io(#[from] io::Error),
  /// TOML 形式解析エラー
  #[error("TOML 形式の解析に失敗しました: {0}")]
  #[diagnostic(code(config::toml_parse), help("TOML 構文が正しいか確認してください。"))]
  Toml(#[from] toml::de::Error),
  /// 出力ディレクトリ作成エラー
  #[error("出力ディレクトリの作成に失敗しました。")]
  #[diagnostic(code(config::create_dir), help("ディレクトリ権限とパスを確認してください。"))]
  CreateDir {
    #[source]
    source: io::Error,
  },
  /// カレント作業ディレクトリ取得エラー
  #[error("カレント作業ディレクトリの取得に失敗しました。")]
  #[diagnostic(code(config::current_dir), help("ファイルシステムの状態を確認してください。"))]
  CurrentDir {
    #[source]
    source: io::Error,
  },
  /// パス正規化エラー
  #[error("パス '{path}' の正規化に失敗しました。")]
  #[diagnostic(
    code(config::canonicalize),
    help("パスが存在するか、またはシンボリックリンクが有効であるか確認してください。")
  )]
  Canonicalize {
    path: PathBuf,
    #[source]
    source: io::Error,
  },
  /// 複合バリデーションエラー
  #[error("複数のバリデーションエラーが発生しました。")]
  #[diagnostic(code(config::multiple_validation_errors))]
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
  #[error("'{field}' は正の値である必要があります。")]
  #[diagnostic(code(config::validation::non_positive), help("0 より大きい値を指定してください。"))]
  NonPositive { field: &'static str },
  /// 余白が負の値
  #[error("'{field}' は 0 以上である必要があります。")]
  #[diagnostic(code(config::validation::negative_margin), help("負でない値を指定してください。"))]
  NegativeMargin { field: &'static str },
  /// 余白の合計が寸法以上
  #[error("方向 {axis} の余白合計 ({sum}) が寸法 {limit} 未満である必要があります。")]
  #[diagnostic(code(config::validation::margin_sum), help("余白の合計が寸法未満になるように調整してください。"))]
  MarginSumTooLarge {
    axis: &'static str,
    sum: f32,
    limit: f32,
  },
  /// `font_name`の重複
  #[error("フォント名 '{font_name}' が重複しています。")]
  #[diagnostic(
    code(config::validation::duplicate_font),
    help("各フォント種別に異なるフォント名を指定してください。")
  )]
  DuplicateFontName { font_name: String },
  /// 背景色の範囲エラー
  #[error("背景色 {field} は [0.0, 1.0] の範囲である必要があります。指定値: {value}")]
  #[diagnostic(code(config::validation::background_color), help("0.0 から 1.0 の間の値を指定してください。"))]
  InvalidBackgroundColor { field: &'static str, value: f32 },
  /// バリアブルフォントタグの長さエラー
  #[error("フォント軸名は 4 文字である必要があります。指定値: '{axis_name}'")]
  #[diagnostic(
    code(config::validation::font_axis_length),
    help("OpenType 軸タグ（例：'wght'、'wdth'）として 4 文字を指定してください。")
  )]
  InvalidFontVariationAxisName { axis_name: String },
  /// scriptコードの長さエラー
  #[error("スクリプトコードは 4 文字である必要があります。指定値: '{code}'")]
  #[diagnostic(
    code(config::validation::script_code),
    help("ISO 15924 スクリプトコード（例：'latn'、'arab'）として 4 文字を指定してください。")
  )]
  InvalidScriptCodeLength { code: String },
  /// languageコードの長さエラー
  #[error("言語コードは 3 または 4 文字である必要があります。指定値: '{code}'")]
  #[diagnostic(
    code(config::validation::language_code),
    help("BCP 47 言語タグ（例：'eng'、'ja'）を指定してください。")
  )]
  InvalidLanguageCodeLength { code: String },
  /// featureタグの長さエラー
  #[error("フィーチャータグは 4 文字である必要があります。指定値: '{tag}'")]
  #[diagnostic(
    code(config::validation::font_feature_tag),
    help("OpenType フィーチャータグ（例：'liga'、'smcp'）として 4 文字を指定してください。")
  )]
  InvalidFontFeatureTagLength { tag: String },
}

/// デフォルトパスから設定ファイルを読み込みます
///
/// `./config/config.toml` から TOML 設定を読み込み、
/// パスの解決、バリデーション、型変換を行って
/// アプリケーション使用用の構造化設定に変換します。
///
/// # Returns
///
/// 検証済みの設定情報 [`Config`]
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - ファイルが見つからない、または読み込み権限がない
/// - TOML 構文エラー
/// - バリデーション失敗（サイズ、余白、フォント設定など）
/// - 出力ディレクトリの作成に失敗
/// - パスの正規化に失敗
pub fn read_config_file() -> Result<Config, Report> {
  let config_path = DEFAULT_CONFIG_PATH;
  info!(config_path = %config_path, "Reading config file");
  return read_config_file_with_path(config_path).map_err(std::convert::Into::into);
}

/// 指定されたパスから設定ファイルを読み込みます
///
/// TOML ファイルをパース後、以下の処理を実行します：
/// 1. ファイルパスの解決と正規化（`canonicalize`）
/// 2. ページサイズ、フォント、余白のバリデーション
/// 3. 出力ディレクトリの作成
/// 4. フォント設定の型変換（スクリプト、言語、バリアブル軸など）
///
/// # Arguments
///
/// * `config_path` - 設定ファイルのパス（相対・絶対いずれでも可）
///
/// # Returns
///
/// 検証済みの設定情報 [`Config`]
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - ファイルが見つからない、または読み込み権限がない場合
/// - TOML 構文エラー
/// - バリデーション失敗（複数のエラーをまとめて報告）
/// - 出力ディレクトリの作成に失敗
/// - パス正規化に失敗
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

/// プリフォント設定を処理済みフォント設定に変換します
///
/// TOML からパースされた `PreFontConfig` を、
/// アプリケーションが直接使用できる `FontConfig` に変換します。
/// この過程でスクリプト、言語、フィーチャーをバリデーションして変換します。
///
/// # Arguments
///
/// * `pre_font_config` - パース済み設定
/// * `errors` - バリデーションエラー蓄積用ベクタ
///
/// # Returns
///
/// 変換済みフォント設定
///
/// # Errors
///
/// ファイルパスの正規化に失敗した場合
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

/// ファイルパスを正規化された絶対パスに解決します
///
/// 相対パスを絶対パスに変換し、シンボリックリンクを解決して、
/// 正規化された絶対パスを返します。この関数はファイルが存在することを前提としています。
///
/// # Arguments
///
/// * `path` - 解決するパス（相対パスまたは絶対パス）
///
/// # Returns
///
/// 正規化された絶対パス
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - ファイルが存在しない場合
/// - シンボリックリンク解決に失敗した場合
/// - パスの正規化に失敗した場合（権限エラーなど）
fn resolve_path_buf(path: PathBuf) -> Result<PathBuf, ReadConfigError> {
  let resolved = path.canonicalize().map_err(|error| ReadConfigError::Canonicalize {
    path,
    source: error,
  })?;
  return Ok(resolved);
}

// variation_axes の変換
/// バリアブルフォント軸の設定情報を変換します
///
/// 各軸の名前が 4 文字であることを検証し、`[u8; 4]` 配列に変換します。
/// 不正な長さの軸名はエラーに記録されますが、処理は続行します。
///
/// # Arguments
///
/// * `axes` - 変換対象のバリアブル軸オプション
/// * `errors` - バリデーションエラー蓄積用ベクタ
///
/// # Returns
///
/// 変換済みのバリアブル軸リスト（軸設定がない場合は `None`）
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

/// OpenType スクリプトコードを `[u8; 4]` に変換します
///
/// 4 文字の OpenType スクリプトコード
/// （例："latn" = Latin、"arab" = Arabic、"cyrl" = Cyrillic）
/// を `[u8; 4]` 配列に変換します。
/// 長さが 4 文字でない場合はエラーに記録されます。
///
/// # Arguments
///
/// * `input` - スクリプトコード（4 文字の `String`）
/// * `errors` - バリデーションエラー蓄積用ベクタ
///
/// # Returns
///
/// 変換済みスクリプトコード配列（不正な場合は `None`）
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

/// BCP 47 言語コードを `[u8; 4]` に変換します
///
/// BCP 47 言語タグ（3 または 4 文字）を `[u8; 4]` 配列に変換します。
/// 3 文字の場合は末尾にスペース（0x20）を追加して 4 文字にします。
/// 例：
/// - "eng" → [b'e', b'n', b'g', b' ']
/// - "ja" → 不正（3 文字未満）
/// - "zhCN" → [b'z', b'h', b'C', b'N']
///
/// # Arguments
///
/// * `input` - 言語コード（3 または 4 文字の `String`）
/// * `errors` - バリデーションエラー蓄積用ベクタ
///
/// # Returns
///
/// 変換済み言語コード配列（不正な場合は `None`）
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

/// OpenType フォント機能タグをパースして変換します
///
/// OpenType フィーチャータグ（4 文字）をパースし、
/// タグと値のペアリストに変換します。
/// 不正な長さのタグはエラーに記録されますが、処理は続行します。
///
/// # Arguments
///
/// * `input` - フィーチャー設定オプション
/// * `errors` - バリデーションエラー蓄積用ベクタ
///
/// # Returns
///
/// 変換済みフィーチャーリスト（フィーチャーがない場合は `None`）
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
/// 指定数値フィールドがすべて正であることを検証します
///
/// ページサイズ、フォントサイズ、行の高さ倍率などの
/// 正である必要がある値を検証します。
///
/// # Arguments
///
/// * `fields` - (フィールド名, 値) のペア配列
/// * `errors` - バリデーションエラー蓄積用ベクタ
fn validate_positive_fields(fields: &[(&'static str, f32)], errors: &mut Vec<ValidationError>) {
  for (field, value) in fields {
    if *value <= 0.0 {
      errors.push(ValidationError::NonPositive { field });
    }
  }
}

/// 余白フィールドがすべて非負であることを検証します
///
/// # Arguments
///
/// * `fields` - (フィールド名, 値) のペア配列
/// * `errors` - バリデーションエラー蓄積用ベクタ
fn validate_non_negative_margins(fields: &[(&'static str, f32)], errors: &mut Vec<ValidationError>) {
  for (field, value) in fields {
    if *value < 0.0 {
      errors.push(ValidationError::NegativeMargin { field });
    }
  }
}

/// 余白の合計がページサイズ未満であることを検証します
///
/// 上下左右の余白合計がページのそれぞれの寸法より小さいことを確認します。
/// そうでなければ、テキスト配置領域がなくなるため不正です。
///
/// # Arguments
///
/// * `height` - ページ高さ
/// * `width` - ページ幅
/// * `margin_top` - 上余白
/// * `margin_bottom` - 下余白
/// * `margin_left` - 左余白
/// * `margin_right` - 右余白
/// * `errors` - バリデーションエラー蓄積用ベクタ
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

/// 出力 PDF の絶対パスを生成します（ディレクトリ作成・正規化含む）
///
/// 出力ディレクトリを作成・正規化した上で、
/// PDF ファイル名（`{name}.pdf`）を含むフルパスを生成します。
///
/// # Arguments
///
/// * `current_dir` - カレント作業ディレクトリ
/// * `output_dir` - 設定で指定された出力ディレクトリ
/// * `name` - PDF ファイル名（拡張子なし）
///
/// # Returns
///
/// 正規化された出力 PDF への絶対パス
///
/// # Errors
///
/// ディレクトリ作成またはパス正規化に失敗した場合
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

/// 背景色を検証し RGB タプルを生成します
///
/// R、G、B の各成分を検証（0.0〜1.0 の範囲内）し、
/// 全てそろっていれば RGB タプルを返します。
///
/// # Arguments
///
/// * `r` - 赤成分の設定値
/// * `g` - 緑成分の設定値
/// * `b` - 青成分の設定値
/// * `errors` - バリデーションエラー蓄積用ベクタ
///
/// # Returns
///
/// 検証済み RGB タプル（値がない場合は `None`）
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

/// 19 フォント種別中の `font_name` 重複を検出します
///
/// すべてのフォント種別の `font_name` を調査し、
/// 重複がある場合はエラーに記録します。
///
/// # Arguments
///
/// * `fonts` - 全フォント設定
/// * `errors` - バリデーションエラー蓄積用ベクタ
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
