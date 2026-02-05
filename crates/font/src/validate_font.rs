use font_types::{Fixed, Tag};
use read_config_file::{FontConfig, VariationAxis};
use read_fonts::{FontRef, ReadError, TableProvider, tables::layout::ScriptList};
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum FontValidationError {
  /// フォントファイル読み込み失敗
  #[error("Failed to read font file: {0}")]
  Io(#[from] std::io::Error),
  /// フェース解析失敗
  #[error("Failed to parse font face: {0}")]
  Parse(#[from] read_fonts::ReadError),
  /// バリアブルフォントではないのに軸設定が与えられた
  #[error("Font is not a variable font, but variation axes were provided in config")]
  NotVariableFont,
  /// バリアブルフォントに必要な軸設定が不足
  #[error("Variable font requires variation axes in config")]
  MissingVariationAxes,
  /// 未知の軸名
  #[error("Unknown variation axis: {0}")]
  UnknownVariationAxis(String),
  /// 軸値が許容範囲外
  #[error("Variation value out of range for axis '{name}': {value} (allowed: {min}..={max})")]
  VariationValueOutOfRange {
    name: String,
    min: f64,
    max: f64,
    value: f64,
  },
  /// フォントに存在する軸が設定されていない
  #[error("Font has variation axis '{axis}' that is not configured (default: {default}, min: {min}, max: {max})")]
  UnconfiguredVariationAxis {
    axis: String,
    default: f64,
    min: f64,
    max: f64,
  },
}

/// フォント設定を検証
///
/// # Errors
///
/// フォントの読み込み、解析、またはバリエーション軸検証に失敗した場合にエラーを返します。
pub fn validate_font(config: &FontConfig) -> Result<(), FontValidationError> {
  let font_path = &config.font_path;
  let font_data = std::fs::read(font_path).map_err(FontValidationError::Io)?;
  let font_ref = FontRef::from_index(&font_data, 0).map_err(FontValidationError::Parse)?;

  let variation_axes = &config.variation_axes;
  if let Some(variation_axes) = variation_axes {
    validate_variation_axes(&font_ref, variation_axes)?;
  } else if font_ref.fvar().is_ok() {
    return Err(FontValidationError::MissingVariationAxes);
  }
  if config.script.is_none() && config.language.is_some() {
    warn!("Warning: 'language' is specified without 'script'. 'language' will be ignored.");
  } else {
    check_script_language_support(&font_ref, config);
  }

  Ok(())
}

/// バリアブルフォントの軸を検証
///
/// 設定されたバリエーション軸がフォントに存在する軸と一致し、
/// 値が許容範囲内にあることを確認します。
/// また、フォントに存在するすべての軸が設定されていることも検証します。
/// 複数の軸を持つフォントに対応しています。
///
/// # 引数
///
/// * `ttf_face` - `TrueType`フェース
/// * `config_variation_axes` - 設定されたバリエーション軸
///
/// # エラー
///
/// 未知の軸名、値が範囲外、またはフォントに存在する軸が設定に含まれていない場合にエラーを返します。
fn validate_variation_axes(
  font_ref: &FontRef,
  config_variation_axes: &[VariationAxis],
) -> Result<(), FontValidationError> {
  // フォントのすべての軸を一度取得してキャッシュ
  let fvar = font_ref.fvar().map_err(|_| FontValidationError::NotVariableFont)?;
  let font_axes = fvar.axes().map_err(FontValidationError::Parse)?;

  // 設定された各軸を検証
  for cfg_axis in config_variation_axes {
    let cfg_tag = font_types::Tag::new(&cfg_axis.name);
    let axis = font_axes.iter().find(|axis| axis.axis_tag() == cfg_tag).ok_or_else(|| {
      let name: String = cfg_tag.to_string();
      FontValidationError::UnknownVariationAxis(name)
    })?;

    let min_value = axis.min_value();
    let max_value = axis.max_value();
    if !(min_value..=max_value).contains(&Fixed::from_f64(cfg_axis.value)) {
      let name: String = cfg_tag.to_string();
      return Err(FontValidationError::VariationValueOutOfRange {
        name,
        min: axis.min_value().to_f64(),
        max: axis.max_value().to_f64(),
        value: cfg_axis.value,
      });
    }
  }

  // フォントに存在する軸がすべて設定されているか確認
  for font_axis in font_axes {
    let axis_name: String = font_axis.axis_tag().to_string();
    let is_configured = config_variation_axes
      .iter()
      .any(|cfg_axis| font_types::Tag::new(&cfg_axis.name) == font_axis.axis_tag());

    if !is_configured {
      return Err(FontValidationError::UnconfiguredVariationAxis {
        axis: axis_name,
        default: font_axis.default_value().to_f64(),
        min: font_axis.min_value().to_f64(),
        max: font_axis.max_value().to_f64(),
      });
    }
  }

  Ok(())
}

/// スクリプトと言語のサポートを検証
///
/// フォントのGSUBおよびGPOSテーブルで、指定されたスクリプトと言語が
/// サポートされているかを確認します。サポートされていない場合は警告を出力します。
///
/// # 引数
///
/// * `font_ref` - フォント参照
/// * `font_config` - フォント設定
fn check_script_language_support(font_ref: &FontRef, font_config: &FontConfig) {
  let Some(script) = font_config.script else {
    return;
  };

  let script_tag = Tag::new(&script);
  let lang_tag = font_config.language.map(|lang| Tag::new(&lang));

  // GSUBテーブルのチェック
  if let Ok(gsub) = font_ref.gsub() {
    check_script_in_table(gsub.script_list(), script_tag, lang_tag, "GSUB");
  } else {
    warn!("GSUB table not found.");
  }

  // GPOSテーブルのチェック
  if let Ok(gpos) = font_ref.gpos() {
    check_script_in_table(gpos.script_list(), script_tag, lang_tag, "GPOS");
  } else {
    warn!("GPOS table not found.");
  }
}

/// 指定されたテーブルでスクリプトと言語のサポートを確認
///
/// # 引数
///
/// * `script_list_result` - `ScriptList`の取得結果
/// * `script_tag` - 検証するスクリプトタグ
/// * `lang_tag` - 検証する言語タグ（オプション）
/// * `table_name` - テーブル名（エラーメッセージ用）
fn check_script_in_table(
  script_list_result: Result<ScriptList<'_>, ReadError>,
  script_tag: Tag,
  lang_tag: Option<Tag>,
  table_name: &str,
) {
  let script_list = match script_list_result {
    Ok(list) => list,
    Err(e) => {
      warn!("Failed to read ScriptList from {table_name} table: {e}");
      return;
    },
  };

  let Some(index) = script_list.index_for_tag(script_tag) else {
    warn!("Script '{script_tag}' is NOT supported in {table_name} table.");
    return;
  };

  let script = match script_list.get(index) {
    Ok(record) => record.element,
    Err(_) => return,
  };

  if let Some(lang_tag) = lang_tag
    && script.lang_sys_index_for_tag(lang_tag).is_none()
  {
    warn!("Language '{lang_tag}' is NOT supported under script '{script_tag}' in {table_name} table.");
  }
}
