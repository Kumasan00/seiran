//! フォント設定と OpenType テーブルの検証モジュール
//!
//! フォント設定の妥当性、バリアブルフォント軸設定の完全性、
//! スクリプト・言語のサポート状況などを検証し、
//! PDF 生成前にフォント関連の問題を早期発見します。
//!
//! ## 検証項目
//!
//! ### 1. バリアブルフォント軸検証
//!
//! バリアブルフォントの軸設定に関して以下を検証：
//!
//! - **軸の存在確認**: 設定された軸がフォント内に存在すること
//! - **値の範囲チェック**: 軸値が最小値〜最大値の範囲内であること
//! - **完全性チェック**: フォント内のすべての軸が設定に含まれていること
//!
//! 例えば Weight 軸が [100, 900] の範囲を持つフォントで、
//! 設定値が 950 の場合は範囲外エラーを報告します。
//!
//! ### 2. スクリプト・言語サポート検証
//!
//! フォントの OpenType テーブルで指定されたスクリプト・言語が
//! サポートされているか確認：
//!
//! - **GSUB テーブル**: グリフ置換（字形の変形など）のサポート確認
//! - **GPOS テーブル**: グリフ配置（カーニング、リガチャなど）のサポート確認
//!
//! サポートされていない場合は警告ログを出力（エラーではなく警告）。
//!
//! ## エラー型
//!
//! [`FontValidationError`] は 7 つのエラーバリアントを定義：
//!
//! | エラー | 条件 | 対応 |
//! |-------|------|------|
//! | `Parse` | フォント形式解析失敗 | ファイル形式を確認 |
//! | `NotVariableFont` | 静的フォントに軸設定 | 設定から軸を削除 |
//! | `MissingVariationAxes` | バリアブルに軸設定なし | 設定に軸を追加 |
//! | `UnknownVariationAxis` | 不明な軸名 | `variation-axes` コマンドで確認 |
//! | `VariationValueOutOfRange` | 軸値が範囲外 | 値をクリップまたは調整 |
//! | `UnconfiguredVariationAxis` | フォント内の軸が未設定 | 設定に軸を追加 |
//!
//! ## ライフサイクル
//!
//! ```text
//! FontConfig（設定ファイル）
//!   ↓
//! validate_font()（公開関数）
//!   ↓ + FontRef（フォント）
//! wrapped_validate_font()（内部関数）
//!   ├→ validate_variation_axes()
//!   └→ check_script_language_support()
//!       └→ check_script_in_table()
//!   ↓
//! Result<(), Report>（検証結果）
//! ```
//!
//! ## 検証タイミング
//!
//! 検証は PDF 生成パイプラインの初期段階で実行されます：
//!
//! 1. フォント読み込み（`font::FontData::new`）
//! 2. フォントパース（`font::FontRefs::new`）
//! 3. **→ フォント検証** （このモジュール）
//! 4. メタデータ抽出（`font::font_info::FontInfos::new`）
//! 5. テキスト処理（`parser`）
//! 6. テキストシェイピング（`font::shaper`）
//! 7. PDF 生成（`pdf_gen`）
//!
//! 検証に失敗すると後続の処理は実行されません。
//!
//! ## 警告 vs エラー
//!
//! - **エラー**: 処理を中断する（`FontValidationError` を返す）
//!   - バリアブル軸の検証失敗
//!   - OpenType テーブルの読み込み失敗設定値の矛盾
//!
//! - **警告**: ログ出力のみ、処理は続行（`warn!` マクロ）
//!   - スクリプト・言語がサポートされていない
//!   - 言語が指定されているがスクリプトが未指定
//!
//! ## 使用例
//!
//! ```ignore
//! # use font::validate_font::*;
//! # use read_config::FontConfig;
//! # use read_fonts::FontRef;
//!
//! let config = FontConfig { /* ... */ };
//! let font_ref = FontRef::new(&font_data)?;
//!
//! // 検証を実行
//! validate_font(&config, &font_ref)?;
//!
//! // 検証成功後は PDF 生成処理に進む
//! ```

#![allow(unused_assignments)]

use font_types::{Fixed, Tag};
use miette::Diagnostic;
use read_config::{FontConfig, FontConfigs, VariationAxis};
use read_fonts::{FontRef, ReadError, TableProvider, tables::layout::ScriptList};
use thiserror::Error;
use tracing::{info, warn};
use types::FontType;

use crate::FontRefs;

#[derive(Debug, Error, Diagnostic)]
#[error("複数のフォント設定にエラーがあります")]
#[diagnostic(code(font::validation::multiple_errors))]
struct MultipleFontValidationErrors {
  #[related]
  errors: Vec<FontValidationErrors>,
}

#[derive(Debug, Error, Diagnostic)]
#[error("フォントの検証に失敗しました: {font_type:?}")]
#[diagnostic(code(font::validation::error))]
struct FontValidationErrors {
  font_type: FontType,
  #[related]
  errors: Vec<FontValidationError>,
}

/// フォント検証に関連するエラー
///
/// フォント形式のパース、バリアブルフォント軸の検証、
/// スクリプト/言語サポートの検証などで発生するエラーを表します。
#[derive(Debug, Error, Diagnostic)]
pub enum FontValidationError {
  /// OpenType フォント形式解析エラー
  #[error("フォントフェースの解析に失敗しました: {0}")]
  #[diagnostic(
    code(font::validation::parse),
    help("フォントファイルが破損していないか、正しい形式であるか確認してください。")
  )]
  Parse(#[from] read_fonts::ReadError),
  /// 静的フォントにバリアブル軸設定が指定されたエラー
  #[error("このフォントはバリアブルフォントではありません。設定ファイルにバリエーション軸が指定されています。")]
  #[diagnostic(
    code(font::validation::not_variable_font),
    help("バリアブル対応ではないフォントの場合は、設定ファイルから 'variation_axes' を削除してください。")
  )]
  NotVariableFont,
  /// バリアブルフォント軸設定不足エラー
  #[error("バリアブルフォントにはバリエーション軸の設定が必須です。")]
  #[diagnostic(
    code(font::validation::missing_variation_axes),
    help(
      "設定ファイルに 'variation_axes' セクションを追加してください。'variation-axes' コマンドで利用可能な軸を確認できます。"
    )
  )]
  MissingVariationAxes,
  /// 不明なバリエーション軸名エラー
  #[error("不明なバリエーション軸: {0}")]
  #[diagnostic(
    code(font::validation::unknown_axis),
    help("'variation-axes' コマンドでフォントがサポートする軸を確認してください。")
  )]
  UnknownVariationAxis(String),
  /// バリアブル軸値が許容範囲外エラー
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
  /// フォント内の軸が設定に含まれていないエラー
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

/// すべてのフォント設定を検証します
///
/// # Arguments
///
/// * `font_configs` - フォント設定情報
/// * `font_refs` - フォント参照
///
/// # Returns
///
/// すべてのフォント設定が妥当な場合は `Ok(())`
///
/// # Errors
///
/// 任意のフォント設定の検証に失敗した場合は、`FontValidationError` のバリアントを含む `miette::Report` を返します。
pub fn validate_fonts(font_configs: &FontConfigs, font_refs: &FontRefs) -> miette::Result<()> {
  let mut all_errors = Vec::new();
  for font_type in FontType::ALL {
    let config = font_configs.get(font_type);
    let font_ref = font_refs.get(font_type);
    let errors = validate_font(config, font_ref);
    if !errors.is_empty() {
      all_errors.push(FontValidationErrors { font_type, errors });
    }
    info!(font_type = ?font_type, font_path = %config.font_path.display(), "フォントの検証が完了しました");
  }
  if !all_errors.is_empty() {
    return Err(MultipleFontValidationErrors { errors: all_errors }.into());
  }
  return Ok(());
}

/// フォント設定の詳細な検証を実行します（内部関数）
///
/// # Arguments
///
/// * `config` - フォント設定情報
/// * `font_ref` - OpenType フォント参照
///
/// # Returns
///
/// 検証成功時は `Ok(())`
///
/// # Errors
///
/// 検証に失敗した場合は `FontValidationError` を返します。
pub fn validate_font(config: &FontConfig, font_ref: &FontRef) -> Vec<FontValidationError> {
  let mut errors = Vec::new();
  if let Some(variation_axes) = &config.variation_axes {
    validate_variation_axes(font_ref, variation_axes, &mut errors);
  } else if font_ref.fvar().is_ok() {
    errors.push(FontValidationError::MissingVariationAxes);
  }

  if config.script.is_none() && config.language.is_some() {
    warn!("'language' が 'script' なしで指定されています。'language' は無視されます。");
  } else {
    check_script_language_support(font_ref, config);
  }
  errors
}

/// バリアブルフォントの軸設定を検証します
///
/// 以下の検証を行います：
/// - 設定された軸がフォント内に存在すること
/// - 軸値が最小値から最大値の範囲内であること
/// - フォント内のすべての軸が設定に含まれていること
///
/// # Arguments
///
/// * `font_ref` - フォント参照（fvar テーブル読み込み用）
/// * `config_variation_axes` - 設定ファイルに指定されたバリアブル軸
///
/// # Returns
///
/// すべての軸設定が妥当な場合は `Ok(())`
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - 設定に含まれる軸がフォントに存在しない場合
/// - 軸値が許容範囲外の場合
/// - フォント内の軸が設定に含まれていない場合
fn validate_variation_axes(
  font_ref: &FontRef,
  config_variation_axes: &[VariationAxis],
  errors: &mut Vec<FontValidationError>,
) {
  // fvar テーブルからフォント内のバリアブル軸定義を取得
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

  // 設定されたバリアブル軸ごとにフォント内の定義と照合
  for cfg_axis in config_variation_axes {
    let cfg_tag = Tag::new(&cfg_axis.name);
    let Some(axis) = font_axes.iter().find(|axis| axis.axis_tag() == cfg_tag) else {
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

  // フォント内のバリアブル軸が全て設定ファイルに含まれているか検証
  for font_axis in font_axes {
    let is_configured = config_variation_axes.iter().any(|cfg_axis| Tag::new(&cfg_axis.name) == font_axis.axis_tag());

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

/// スクリプト・言語のサポート状況を確認します
///
/// フォントの GSUB（グリフ置換）および GPOS（グリフ配置）テーブルで、
/// 指定されたスクリプトと言語がサポートされているかを確認します。
/// サポートされていない場合は警告ログを出力します。
///
/// # Arguments
///
/// * `font_ref` - フォント参照
/// * `font_config` - フォント設定情報（スクリプト、言語を含む）
fn check_script_language_support(font_ref: &FontRef, font_config: &FontConfig) {
  let Some(script) = font_config.script else {
    return;
  };

  let script_tag = Tag::new(&script);
  let lang_tag = font_config.language.map(|lang| Tag::new(&lang));

  // GSUB（グリフ置換）テーブルを確認
  if let Ok(gsub) = font_ref.gsub() {
    check_script_in_table(gsub.script_list(), script_tag, lang_tag, "GSUB");
  } else {
    warn!("GSUB テーブルが見つかりません。");
  }

  // GPOS（グリフ配置）テーブルを確認
  if let Ok(gpos) = font_ref.gpos() {
    check_script_in_table(gpos.script_list(), script_tag, lang_tag, "GPOS");
  } else {
    warn!("GPOS テーブルが見つかりません。");
  }
}

/// 特定のテーブル内でスクリプト・言語のサポートを確認します
///
/// GSUB または GPOS テーブルの `ScriptList` から、
/// 指定されたスクリプトと言語がサポートされているかを調べます。
///
/// # Arguments
///
/// * `script_list_result` - `ScriptList` の取得結果
/// * `script_tag` - 確認するスクリプトタグ
/// * `lang_tag` - 確認する言語タグ（オプション）
/// * `table_name` - テーブル名（"GSUB" または "GPOS"）
fn check_script_in_table(
  script_list_result: Result<ScriptList<'_>, ReadError>,
  script_tag: Tag,
  lang_tag: Option<Tag>,
  table_name: &str,
) {
  let script_list = match script_list_result {
    Ok(list) => list,
    Err(e) => {
      warn!("ScriptList の読み込みに失敗しました ({table_name}): {e}");
      return;
    },
  };

  let Some(index) = script_list.index_for_tag(script_tag) else {
    warn!("スクリプト '{script_tag}' は {table_name} テーブルではサポートされていません。");
    return;
  };

  let script = match script_list.get(index) {
    Ok(record) => record.element,
    Err(_) => return,
  };

  if let Some(lang_tag) = lang_tag
    && script.lang_sys_index_for_tag(lang_tag).is_none()
  {
    warn!("言語 '{lang_tag}' はスクリプト '{script_tag}' の下では {table_name} テーブルでサポートされていません。");
  }
}
