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
pub use processed_config::{Config, FontConfig, PdfConfig};

/// 設定ファイル読み込みに関連するエラー
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
    }
  }
}

impl std::error::Error for ReadConfigError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      ReadConfigError::Io(e) | ReadConfigError::CreateDir(e) | ReadConfigError::CurrentDir(e) => {
        Some(e)
      }
      ReadConfigError::Toml(e) => Some(e),
      ReadConfigError::InvalidValue { .. }
      | ReadConfigError::NonPositive { .. }
      | ReadConfigError::NegativeMargin { .. }
      | ReadConfigError::MarginSumTooLarge { .. } => None,
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
  config_file_path: P,
) -> Result<processed_config::Config, ReadConfigError> {
  let config_content = fs::read_to_string(config_file_path)?;
  let pre_config: PreConfig = toml::from_str(&config_content)?;
  let current_dir = std::env::current_dir().map_err(ReadConfigError::CurrentDir)?;

  // Destructure to move values without extra clones
  let pre_config::PreConfig {
    name,
    pdf: pre_pdf,
    main_font: pre_main_font,
    main_japanese_font: pre_main_jp_font,
    math_font: pre_math_font,
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

  let pre_config::PreFontConfig {
    font_name: main_font_name,
    font_path: main_font_path,
    font_index: main_font_index,
    variation_axes: main_font_variation_axes,
  } = pre_main_font;

  let pre_config::PreMathFontConfig {
    font_name: math_font_name,
    font_path: math_font_path,
    font_index: math_font_index,
  } = pre_math_font;

  let main_font_variation_axes = main_font_variation_axes.map(|axes| {
    axes
      .into_iter()
      .map(|axis| processed_config::VariationAxis {
        name: axis.name,
        value: axis.value,
      })
      .collect()
  });

  let pre_config::PreFontConfig {
    font_name: main_jp_font_name,
    font_path: main_jp_font_path,
    font_index: main_jp_font_index,
    variation_axes: main_jp_font_variation_axes,
  } = pre_main_jp_font;

  let main_jp_font_variation_axes = main_jp_font_variation_axes.map(|axes| {
    axes
      .into_iter()
      .map(|axis| processed_config::VariationAxis {
        name: axis.name,
        value: axis.value,
      })
      .collect()
  });

  let height: f32 = height;
  let width: f32 = width;
  if height <= 0.0 {
    return Err(ReadConfigError::NonPositive {
      field: "pdf.height",
    });
  }
  if width <= 0.0 {
    return Err(ReadConfigError::NonPositive { field: "pdf.width" });
  }
  if font_size <= 0.0 {
    return Err(ReadConfigError::NonPositive {
      field: "pdf.font_size",
    });
  }

  if line_height_factor <= 0.0 {
    return Err(ReadConfigError::NonPositive {
      field: "pdf.line_height_factor",
    });
  }

  // margin のチェック（負の値は不可）
  if margin_top < 0.0 {
    return Err(ReadConfigError::NegativeMargin {
      field: "pdf.margin_top",
    });
  }
  if margin_bottom < 0.0 {
    return Err(ReadConfigError::NegativeMargin {
      field: "pdf.margin_bottom",
    });
  }
  if margin_left < 0.0 {
    return Err(ReadConfigError::NegativeMargin {
      field: "pdf.margin_left",
    });
  }
  if margin_right < 0.0 {
    return Err(ReadConfigError::NegativeMargin {
      field: "pdf.margin_right",
    });
  }

  // margin の合計が高さ・幅を超えないか（厳密に小さいこと）
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

  let output_dir_path = resolve_path(&current_dir, &output_dir);
  if let Some(parent) = output_dir_path.parent() {
    // Create intermediate directories for the output dir if needed
    fs::create_dir_all(parent).map_err(ReadConfigError::CreateDir)?;
  }
  // Ensure output directory itself exists
  fs::create_dir_all(&output_dir_path).map_err(ReadConfigError::CreateDir)?;

  let mut output_path = output_dir_path.join(&name);
  output_path.set_extension("pdf");

  let main_font_path = resolve_path(&current_dir, &main_font_path);
  let main_jp_font_path = resolve_path(&current_dir, &main_jp_font_path);
  let math_font_path = resolve_path(&current_dir, &math_font_path);

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
    main_font: processed_config::FontConfig {
      font_name: main_font_name,
      font_path: main_font_path,
      font_index: main_font_index,
      variation_axes: main_font_variation_axes,
    },
    main_japanese_font: processed_config::FontConfig {
      font_name: main_jp_font_name,
      font_path: main_jp_font_path,
      font_index: main_jp_font_index,
      variation_axes: main_jp_font_variation_axes,
    },
    math_font: processed_config::MathFontConfig {
      font_name: math_font_name,
      font_path: math_font_path,
      font_index: math_font_index,
    },
  };
  Ok(config)
}

/// 相対パスを絶対パスに解決
///
/// パスが相対パスの場合は、ベースパスに結合します。
/// 絶対パスの場合はそのまま返します。
///
/// # 引数
///
/// * `base` - ベースディレクトリ
/// * `p` - 解決するパス
fn resolve_path(base: &Path, p: impl AsRef<Path>) -> PathBuf {
  let p = p.as_ref();
  if p.is_absolute() {
    p.to_path_buf()
  } else {
    base.join(p)
  }
}
