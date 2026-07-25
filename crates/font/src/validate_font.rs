//! フォント設定と OpenType テーブルの検証モジュール
//!
//! バリエーション軸設定の存在・範囲・完全性を検証する。GSUB/GPOS の
//! スクリプト・言語サポート不足は処理を止めず、警告として報告する。

use config::{FontConfig, FontConfigs, VariationAxis};
use font_types::{Fixed, Tag};
use miette::Diagnostic;
use model::FontType;
use read_fonts::{FontRef, ReadError, TableProvider, tables::layout::ScriptList};
use thiserror::Error;
use tracing::{debug, warn};

use crate::FontRefs;

/// 複数のフォント種別で発生した検証エラー。
#[derive(Debug, Error, Diagnostic)]
#[error("複数のフォント設定にエラーがあります")]
#[diagnostic(code(font::validation::multiple_errors))]
pub struct MultipleFontValidationErrors {
  /// フォント種別ごとに集約された検証エラー
  #[related]
  errors: Vec<FontValidationErrors>,
}

/// 1 フォント種別で発生した検証エラー。
#[derive(Debug, Error, Diagnostic)]
#[error("フォントの検証に失敗しました: {font_type:?}")]
#[diagnostic(code(font::validation::error))]
pub struct FontValidationErrors {
  /// 検証対象のフォント種別
  font_type: FontType,
  /// この種別で発生した個別の検証エラー
  #[related]
  errors: Vec<FontValidationError>,
}

/// フォント設定の検証エラー。
#[derive(Debug, Error, Diagnostic)]
pub enum FontValidationError {
  /// OpenType フォントを解析できない。
  #[error("フォントフェースの解析に失敗しました: {0}")]
  #[diagnostic(
    code(font::validation::parse),
    help("フォントファイルが破損していないか、正しい形式であるか確認してください。")
  )]
  Parse(#[from] read_fonts::ReadError),
  /// 静的フォントにバリエーション軸が設定されている。
  #[error("このフォントはバリアブルフォントではありません。設定ファイルにバリエーション軸が指定されています。")]
  #[diagnostic(
    code(font::validation::not_variable_font),
    help("バリアブル対応ではないフォントの場合は、設定ファイルから 'variation_axes' を削除してください。")
  )]
  NotVariableFont,
  /// バリアブルフォントに軸設定がない。
  #[error("バリアブルフォントにはバリエーション軸の設定が必須です。")]
  #[diagnostic(
    code(font::validation::missing_variation_axes),
    help(
      "設定ファイルに 'variation_axes' セクションを追加してください。'variation-axes' コマンドで利用可能な軸を確認できます。"
    )
  )]
  MissingVariationAxes,
  /// フォントに存在しない軸が設定されている。
  #[error("不明なバリエーション軸: {0}")]
  #[diagnostic(
    code(font::validation::unknown_axis),
    help("'variation-axes' コマンドでフォントがサポートする軸を確認してください。")
  )]
  UnknownVariationAxis(String),
  /// 軸値が許容範囲外にある。
  #[error("軸 '{name}' の値が範囲外です: {value} (許容範囲: {min}..={max})")]
  #[diagnostic(code(font::validation::value_out_of_range), help("値をフォントの許容範囲内に設定してください。"))]
  VariationValueOutOfRange {
    /// 軸名
    name: String,
    /// 最小値
    min: Fixed,
    /// 最大値
    max: Fixed,
    /// 指定された値
    value: f64,
  },
  /// フォントが持つ軸の設定がない。
  #[error("フォントのバリエーション軸 '{axis}' が設定されていません (デフォルト: {default}, 最小: {min}, 最大: {max})")]
  #[diagnostic(
    code(font::validation::unconfigured_axis),
    help("設定ファイルの 'variation_axes' にこの軸を追加してください。")
  )]
  UnconfiguredVariationAxis {
    /// フォント内の軸名
    axis: String,
    /// デフォルト値
    default: Fixed,
    /// 最小値
    min: Fixed,
    /// 最大値
    max: Fixed,
  },
}

/// 全フォント種別を検証し、違反を種別ごとに集約する。
///
/// # Errors
///
/// 1 つ以上の違反がある場合に [`MultipleFontValidationErrors`] を返す。
pub fn validate_fonts(font_configs: &FontConfigs, font_refs: &FontRefs) -> Result<(), MultipleFontValidationErrors> {
  let mut all_errors = Vec::new();
  for font_type in FontType::ALL {
    let config = font_configs.get(font_type);
    let font_ref = font_refs.get(font_type);
    let errors = validate_font(config, font_ref);
    if !errors.is_empty() {
      all_errors.push(FontValidationErrors { font_type, errors });
    }
    debug!(font_type = ?font_type, font_path = %config.font_path.display(), "フォントを検証しました");
  }
  if !all_errors.is_empty() {
    return Err(MultipleFontValidationErrors { errors: all_errors });
  }
  return Ok(());
}

/// 1 フォント分を検証し、検出した違反をすべて返す。
#[must_use]
pub fn validate_font(config: &FontConfig, font_ref: &FontRef) -> Vec<FontValidationError> {
  let mut errors = Vec::new();
  if let Some(variation_axes) = &config.variation_axes {
    validate_variation_axes(font_ref, variation_axes, &mut errors);
  } else if font_ref.fvar().is_ok() {
    errors.push(FontValidationError::MissingVariationAxes);
  }

  check_script_language_support(font_ref, config);
  return errors;
}

/// バリエーション軸の存在・値域・設定漏れを検証する。
fn validate_variation_axes(
  font_ref: &FontRef,
  config_variation_axes: &[VariationAxis],
  errors: &mut Vec<FontValidationError>,
) {
  let Ok(fvar) = font_ref.fvar() else {
    errors.push(FontValidationError::NotVariableFont);
    return;
  };
  let font_axes = match fvar.axes() {
    Ok(axes) => axes,
    Err(e) => {
      errors.push(FontValidationError::Parse(e));
      return;
    },
  };

  for cfg_axis in config_variation_axes {
    let cfg_tag = Tag::new(&cfg_axis.name);
    let Some(axis) = font_axes.iter().find(|axis| return axis.axis_tag() == cfg_tag) else {
      errors.push(FontValidationError::UnknownVariationAxis(cfg_tag.to_string()));
      continue;
    };

    let min_value = axis.min_value();
    let max_value = axis.max_value();
    if !(min_value..=max_value).contains(&Fixed::from_f64(cfg_axis.value)) {
      errors.push(FontValidationError::VariationValueOutOfRange {
        name: cfg_tag.to_string(),
        min: min_value,
        max: max_value,
        value: cfg_axis.value,
      });
    }
  }

  for font_axis in font_axes {
    let is_configured =
      config_variation_axes.iter().any(|cfg_axis| return Tag::new(&cfg_axis.name) == font_axis.axis_tag());

    if !is_configured {
      errors.push(FontValidationError::UnconfiguredVariationAxis {
        axis: font_axis.axis_tag().to_string(),
        default: font_axis.default_value(),
        min: font_axis.min_value(),
        max: font_axis.max_value(),
      });
    }
  }
}

/// GSUB/GPOS で設定されたスクリプトと言語のサポートを確認する。
///
/// 言語は `ot_language` が明示された場合だけ確認し、BCP 47 からの導出は `harfrust` に委ねる。
fn check_script_language_support(font_ref: &FontRef, font_config: &FontConfig) {
  let Some(script) = font_config.script else {
    return;
  };

  let script_tag = Tag::new(&script);
  let lang_tag = font_config.ot_language_tag.map(|lang| return Tag::new(&lang));

  if let Ok(gsub) = font_ref.gsub() {
    check_script_in_table(gsub.script_list(), script_tag, lang_tag, "GSUB");
  } else {
    warn!(table_name = "GSUB", "テーブルが見つかりません");
  }

  if let Ok(gpos) = font_ref.gpos() {
    check_script_in_table(gpos.script_list(), script_tag, lang_tag, "GPOS");
  } else {
    warn!(table_name = "GPOS", "テーブルが見つかりません");
  }
}

/// GSUB または GPOS の `ScriptList` でスクリプトと言語を確認する。
fn check_script_in_table(
  script_list_result: Result<ScriptList<'_>, ReadError>,
  script_tag: Tag,
  lang_tag: Option<Tag>,
  table_name: &str,
) {
  let script_list = match script_list_result {
    Ok(list) => list,
    Err(e) => {
      warn!(table_name, error = %e, "ScriptList の読み込みに失敗しました");
      return;
    },
  };

  let Some(index) = script_list.index_for_tag(script_tag) else {
    warn!(script_tag = %script_tag, table_name, "スクリプトがテーブルでサポートされていません");
    return;
  };

  let script = match script_list.get(index) {
    Ok(record) => record.element,
    Err(_) => return,
  };

  if let Some(lang_tag) = lang_tag
    && script.lang_sys_index_for_tag(lang_tag).is_none()
  {
    warn!(lang_tag = %lang_tag, script_tag = %script_tag, table_name, "言語がスクリプト配下でテーブルにサポートされていません");
  }
}
