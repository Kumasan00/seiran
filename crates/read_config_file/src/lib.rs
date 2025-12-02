//! 設定ファイル読み込みモジュール
//!
//! このモジュールは、TOML形式の設定ファイルを読み込み、
//! アプリケーションが使用する設定情報に変換します。

use std::{
  fs, io,
  path::{Path, PathBuf},
};

mod pre_config;
use pre_config::PreConfig;
mod processed_config;

// processed_config の型を公開
pub use processed_config::{Config, FontConfig, PdfConfig, VariationAxis};

/// 設定ファイル読み込みに関連するエラー
///
/// ファイルI/O、TOML解析、設定値のバリデーションに関する
/// 様々なエラーケースを表現します。
#[derive(Debug)]
pub enum ReadConfigError {
  /// 入出力エラー
  Io(io::Error),
  /// TOML解析エラー
  Toml(toml::de::Error),
  /// 出力ディレクトリの作成に失敗
  CreateDir(io::Error),
  /// 無効な設定値（一般）
  InvalidValue { field: &'static str, msg: String },
  /// サイズ系が正でない（height/width/font_size/line_height_factor）
  NonPositive { field: &'static str },
  /// 余白が負の値
  NegativeMargin { field: &'static str },
  /// 余白の合計が寸法以上
  MarginSumTooLarge {
    axis: &'static str,
    sum: f32,
    limit: f32,
  },
  /// カレントディレクトリの取得に失敗
  CurrentDir(io::Error),
  /// パスの正規化に失敗
  Canonicalize { path: PathBuf, error: io::Error },
  /// font_nameの重複
  DuplicateFontName { font_name: String },
}

impl std::fmt::Display for ReadConfigError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ReadConfigError::Io(e) => write!(f, "IO error: {}", e),
      ReadConfigError::Toml(e) => write!(f, "TOML parse error: {}", e),
      ReadConfigError::CreateDir(e) => write!(f, "Failed to create output directory: {}", e),
      ReadConfigError::InvalidValue { field, msg } => {
        write!(f, "Invalid value for '{}': {}", field, msg)
      }
      ReadConfigError::NonPositive { field } => {
        write!(f, "'{}' must be > 0", field)
      }
      ReadConfigError::NegativeMargin { field } => {
        write!(f, "'{}' must be >= 0", field)
      }
      ReadConfigError::MarginSumTooLarge { axis, sum, limit } => {
        write!(f, "margin sum for {} ({} ) must be < {}", axis, sum, limit)
      }
      ReadConfigError::CurrentDir(e) => write!(f, "Failed to get current directory: {}", e),
      ReadConfigError::Canonicalize { path, error } => {
        write!(
          f,
          "Failed to canonicalize path '{}': {}",
          path.display(),
          error
        )
      }
      ReadConfigError::DuplicateFontName { font_name } => {
        write!(f, "Duplicate font_name found: '{}'", font_name)
      }
    }
  }
}

impl std::error::Error for ReadConfigError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      ReadConfigError::Io(e)
      | ReadConfigError::CreateDir(e)
      | ReadConfigError::CurrentDir(e)
      | ReadConfigError::Canonicalize { error: e, .. } => Some(e),
      ReadConfigError::Toml(e) => Some(e),
      ReadConfigError::InvalidValue { .. }
      | ReadConfigError::NonPositive { .. }
      | ReadConfigError::NegativeMargin { .. }
      | ReadConfigError::MarginSumTooLarge { .. }
      | ReadConfigError::DuplicateFontName { .. } => None,
    }
  }
}

impl From<io::Error> for ReadConfigError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

impl From<toml::de::Error> for ReadConfigError {
  fn from(value: toml::de::Error) -> Self {
    Self::Toml(value)
  }
}

/// デフォルトパスから設定ファイルを読み込み
///
/// `./config/config.toml`から設定を読み込みます。
///
/// # 戻り値
///
/// 解析された設定情報を返します。
///
/// # エラー
///
/// ファイル読み込みまたは解析に失敗した場合にエラーを返します。
pub fn read_config_file() -> Result<processed_config::Config, ReadConfigError> {
  read_config_file_with_path("./config/config.toml")
}

/// 指定されたパスから設定ファイルを読み込み
///
/// TOMLファイルを読み込み、パスの解決とバリデーションを行います。
///
/// # 引数
///
/// * `config_file_path` - 設定ファイルのパス
///
/// # 戻り値
///
/// 解析された設定情報を返します。
///
/// # エラー
///
/// ファイル読み込み、解析、バリデーションのいずれかで失敗した場合。
pub fn read_config_file_with_path<P: AsRef<Path>>(
  config_path: P,
) -> Result<processed_config::Config, ReadConfigError> {
  let config_content = fs::read_to_string(config_path)?;
  let pre_config: PreConfig = toml::from_str(&config_content)?;
  let current_dir = std::env::current_dir().map_err(ReadConfigError::CurrentDir)?;

  // ヘルパー: PreFontConfig/PreMathFontConfig から FontConfig/MathFontConfig を生成
  fn to_font_config(
    f: pre_config::PreFontConfig,
    axes_conv: fn(
      Option<Vec<pre_config::PreVariationAxis>>,
    ) -> Option<Vec<processed_config::VariationAxis>>,
  ) -> Result<processed_config::FontConfig, ReadConfigError> {
    Ok(processed_config::FontConfig {
      font_name: f.font_name,
      font_path: resolve_path_buf(f.font_path)?,
      font_index: f.font_index,
      variation_axes: axes_conv(f.variation_axes),
    })
  }
  fn to_math_font_config(
    f: pre_config::PreMathFontConfig,
  ) -> Result<processed_config::MathFontConfig, ReadConfigError> {
    Ok(processed_config::MathFontConfig {
      font_name: f.font_name,
      font_path: resolve_path_buf(f.font_path)?,
      font_index: f.font_index,
    })
  }

  // variation_axes の変換
  fn convert_axes(
    axes: Option<Vec<pre_config::PreVariationAxis>>,
  ) -> Option<Vec<processed_config::VariationAxis>> {
    axes.map(|axes| {
      axes
        .into_iter()
        .map(|axis| processed_config::VariationAxis {
          name: axis.name,
          value: axis.value,
        })
        .collect()
    })
  }

  // 構造体分解
  let pre_config::PreConfig {
    name,
    pdf: pre_pdf,
    main_font,
    italic_font,
    math_font,
    main_japanese_font,
    sans_font,
    sans_japanese_font,
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
  } = pre_pdf;

  // バリデーション
  for (field, value) in [
    ("pdf.height", height),
    ("pdf.width", width),
    ("pdf.font_size", font_size),
    ("pdf.line_height_factor", line_height_factor),
  ] {
    if value <= 0.0 {
      return Err(ReadConfigError::NonPositive { field });
    }
  }
  for (field, value) in [
    ("pdf.margin_top", margin_top),
    ("pdf.margin_bottom", margin_bottom),
    ("pdf.margin_left", margin_left),
    ("pdf.margin_right", margin_right),
  ] {
    if value < 0.0 {
      return Err(ReadConfigError::NegativeMargin { field });
    }
  }
  if margin_top + margin_bottom >= height {
    return Err(ReadConfigError::MarginSumTooLarge {
      axis: "vertical",
      sum: margin_top + margin_bottom,
      limit: height,
    });
  }
  if margin_left + margin_right >= width {
    return Err(ReadConfigError::MarginSumTooLarge {
      axis: "horizontal",
      sum: margin_left + margin_right,
      limit: width,
    });
  }

  // 出力ディレクトリ作成とパス解決
  let output_dir_path = if output_dir.is_absolute() {
    output_dir
  } else {
    current_dir.join(output_dir)
  };
  fs::create_dir_all(&output_dir_path).map_err(ReadConfigError::CreateDir)?;
  let mut output_path =
    output_dir_path
      .canonicalize()
      .map_err(|error| ReadConfigError::Canonicalize {
        path: output_dir_path.clone(),
        error,
      })?;
  output_path.push(&name);
  output_path.set_extension("pdf");

  // FontConfig/MathFontConfig 生成
  let config = processed_config::Config {
    name,
    pdf: processed_config::PdfConfig {
      output_path,
      height,
      width,
      font_size,
      line_height_factor,
      margin_top,
      margin_bottom,
      margin_left,
      margin_right,
    },
    main_font: to_font_config(main_font, convert_axes)?,
    italic_font: to_font_config(italic_font, convert_axes)?,
    math_font: to_math_font_config(math_font)?,
    main_japanese_font: to_font_config(main_japanese_font, convert_axes)?,
    sans_font: to_font_config(sans_font, convert_axes)?,
    sans_japanese_font: to_font_config(sans_japanese_font, convert_axes)?,
  };

  // font_nameの重複チェック
  let mut font_names = std::collections::HashSet::new();
  for font_name in [
    &config.main_font.font_name,
    &config.italic_font.font_name,
    &config.math_font.font_name,
    &config.sans_font.font_name,
    &config.main_japanese_font.font_name,
    &config.sans_japanese_font.font_name,
  ] {
    if !font_names.insert(font_name) {
      return Err(ReadConfigError::DuplicateFontName {
        font_name: font_name.clone(),
      });
    }
  }

  Ok(config)
}

/// 相対パスを絶対パスに解決し正規化
///
/// PathBufを直接受け取り、canonicalize()を使用してシンボリックリンクを解決し、
/// 正規化された絶対パスを返します。
///
/// # 引数
///
/// * `p` - 解決するパス(所有権を移動)
///
/// # エラー
///
/// パスの正規化に失敗した場合、または存在しないパスの場合にエラーを返します。
fn resolve_path_buf(path: PathBuf) -> Result<PathBuf, ReadConfigError> {
  path
    .canonicalize()
    .map_err(|error| ReadConfigError::Canonicalize { path, error })
}
