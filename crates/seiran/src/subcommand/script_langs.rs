#![allow(unused_assignments)]
use std::{collections::BTreeSet, fs, path::Path};

use miette::{Diagnostic, IntoDiagnostic};
use read_fonts::{
  FontRef, ReadError, TableProvider,
  tables::layout::{FeatureList, FeatureParams, LangSys, ScriptList},
};
use thiserror::Error;

/// フォントスクリプト/言語システム解析時のエラー
#[derive(Error, Debug, Diagnostic)]
#[allow(clippy::enum_variant_names)]
enum ScriptLangsError {
  /// ファイル読み込みエラー
  #[error("Failed to read font file: {0}")]
  #[diagnostic(code(script_langs::io_error), help("Verify that the file exists and you have read permissions"))]
  IoError(#[from] std::io::Error),
  /// フォント解析エラー
  #[error("Failed to parse font at index {font_index}")]
  #[diagnostic(
    code(script_langs::font_parse_error),
    help("Ensure the file is a valid font file (TTF/OTF/TTC/OTC). For font collections, try a different index.")
  )]
  FontParseError {
    /// フォントインデックス
    font_index: u32,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GSUBテーブル取得エラー
  #[error("GSUB table not found or invalid")]
  #[diagnostic(code(script_langs::gsub_error), help("This font may not contain a GSUB (Glyph Substitution) table"))]
  GsubError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GPOSテーブル取得エラー
  #[error("GPOS table not found or invalid")]
  #[diagnostic(code(script_langs::gpos_error), help("This font may not contain a GPOS (Glyph Positioning) table"))]
  GposError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// `FeatureList`取得エラー
  #[error("Failed to retrieve feature list from {table_name} table")]
  #[diagnostic(code(script_langs::feature_list_error), help("The {table_name} table structure may be corrupted"))]
  FeatureListError {
    /// テーブル名（GSUB/GPOS）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// `ScriptList`取得エラー
  #[error("Failed to retrieve script list from {table_name} table")]
  #[diagnostic(code(script_langs::script_list_error), help("The {table_name} table structure may be corrupted"))]
  ScriptListError {
    /// テーブル名（GSUB/GPOS）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// `LanguageSystem`取得エラー
  #[error("Failed to retrieve language system at index {index} for script '{script_tag}'")]
  #[diagnostic(code(script_langs::lang_sys_error), help("The language system entry may be invalid"))]
  LangSysError {
    /// 言語システムインデックス
    index: u16,
    /// スクリプトタグ
    script_tag: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature取得エラー
  #[error("Failed to retrieve feature at index {index}")]
  #[diagnostic(code(script_langs::feature_error), help("The feature table entry may be invalid"))]
  FeatureError {
    /// フィーチャーインデックス
    index: u16,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// `FeatureParams`取得エラー
  #[error("Failed to retrieve feature parameters for feature '{feature_tag}'")]
  #[diagnostic(code(script_langs::feature_params_error), help("The feature parameters may be corrupted"))]
  FeatureParamsError {
    /// フィーチャータグ
    feature_tag: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
}

/// フォントスクリプト/言語システム表示用のエントリーポイント
///
/// # 引数
///
/// * `file_path` - フォントファイルのパス
/// * `font_index` - TTC/TTCF など複合フォントのインデックス
pub fn script_langs(file_path: &Path, font_index: u32) -> miette::Result<()> {
  let absolute_path = file_path.canonicalize().into_diagnostic()?;
  let mut referenced_features = BTreeSet::new();
  let font_data = fs::read(&absolute_path).into_diagnostic()?;
  let font_ref = FontRef::from_index(&font_data, font_index)
    .map_err(|source| ScriptLangsError::FontParseError { font_index, source })?;

  // GSUB/GPOS テーブルからスクリプト/言語システムと対応フィーチャを出力
  let gsub = font_ref.gsub().map_err(|source| ScriptLangsError::GsubError { source })?;
  let gsub_features = process_layout_table("GSUB", gsub.feature_list(), gsub.script_list(), &mut referenced_features)?;

  let gpos = font_ref.gpos().map_err(|source| ScriptLangsError::GposError { source })?;
  let gpos_features = process_layout_table("GPOS", gpos.feature_list(), gpos.script_list(), &mut referenced_features)?;

  // 統計情報の収集と表示
  let all_features = collect_all_features(&gsub_features, &gpos_features);
  print_feature_statistics(&all_features, &referenced_features);

  return Ok(());
}

/// レイアウトテーブル (GSUB/GPOS) の Script/Language System を出力し `FeatureList` を返す
///
/// # 引数
///
/// * `table_name` - テーブル名 ("GSUB" / "GPOS")
/// * `feature_list` - `FeatureList`
/// * `script_list` - `ScriptList`
/// * `referenced_features` - 参照された Feature を記録するセット
fn process_layout_table<'a>(
  table_name: &'static str,
  feature_list: Result<FeatureList<'a>, ReadError>,
  script_list: Result<ScriptList<'a>, ReadError>,
  referenced_features: &mut BTreeSet<String>,
) -> Result<FeatureList<'a>, ScriptLangsError> {
  println!("{table_name} Table:");
  let feature_list = feature_list.map_err(|source| ScriptLangsError::FeatureListError { table_name, source })?;
  let script_list = script_list.map_err(|source| ScriptLangsError::ScriptListError { table_name, source })?;
  print_scripts(&script_list, &feature_list, referenced_features)?;

  return Ok(feature_list);
}

/// Script/Language System と対応 Feature を出力
///
/// `ScriptList` を走査し、各 Script のデフォルト言語システムと各言語システムに
/// 紐付く Feature のタグ一覧を表示します。
///
/// # 引数
///
/// * `scripts` - GSUB/GPOS の `ScriptList`
/// * `features` - GSUB/GPOS の `FeatureList`
///
/// # 戻り値
///
/// 成功した場合は `Ok(())` を返します。
///
/// # エラー
///
/// テーブル要素の取得に失敗した場合にエラーを返します。
fn print_scripts(
  scripts: &ScriptList,
  features: &FeatureList,
  referenced_features: &mut BTreeSet<String>,
) -> Result<(), ScriptLangsError> {
  for script_record in scripts.script_records() {
    let script_tag = script_record.script_tag().to_string();
    println!("  Script: {script_tag}");
    if let Ok(subtable) = script_record.script(scripts.offset_data()) {
      if let Some(default_lang_sys) = subtable.default_lang_sys() {
        let default_lang = default_lang_sys.map_err(|source| ScriptLangsError::LangSysError {
          index: 0,
          script_tag: script_tag.clone(),
          source,
        })?;
        let feature_tags = get_language_features(&default_lang, features, referenced_features)?;
        println!("    Default Language System: {feature_tags:?}");
      }
      for lang_record in subtable.lang_sys_records() {
        if let Ok(lang_sys) = lang_record.lang_sys(subtable.offset_data()) {
          let lang_tag = lang_record.lang_sys_tag().to_string();
          let feature_tags = get_language_features(&lang_sys, features, referenced_features)?;
          println!("    {lang_tag}: {feature_tags:?}");
        }
      }
    }
  }
  return Ok(());
}

/// GSUB/GPOS テーブルから全 Feature タグを収集
///
/// GSUB と GPOS の両方の `FeatureList` から全ての Feature タグを収集し、
/// 重複を除いた一意なタグのセットを返します。
///
/// # 引数
///
/// * `gsub_features` - GSUB の `FeatureList`
/// * `gpos_features` - GPOS の `FeatureList`
///
/// # 戻り値
///
/// 全ての一意な Feature タグのセットを返します。
///
/// # エラー
///
/// Feature の取得に失敗した場合にエラーを返します。
fn collect_all_features(
  gsub_features: &FeatureList,
  gpos_features: &FeatureList,
) -> std::collections::BTreeSet<std::string::String> {
  let mut all_features = BTreeSet::new();

  insert_feature_tags(gsub_features, &mut all_features);
  insert_feature_tags(gpos_features, &mut all_features);

  return all_features;
}

/// `FeatureList` からタグを抽出しセットへ追加
///
/// # 引数
///
/// * `feature_list` - 抽出対象の `FeatureList`
/// * `all_features` - 収集先のセット
fn insert_feature_tags(feature_list: &FeatureList, all_features: &mut BTreeSet<String>) {
  for record in feature_list.feature_records() {
    let tag = record.feature_tag().to_string();
    all_features.insert(tag);
  }
}

/// Feature の統計情報を出力
///
/// フォント内の全 Feature 数、Script/Language System から参照されている Feature 数、
/// および参照されていない Feature の一覧を標準出力に表示します。
///
/// # 引数
///
/// * `all_features` - 全 Feature のセット
/// * `referenced_features` - 参照されている Feature のセット
fn print_feature_statistics(all_features: &BTreeSet<String>, referenced_features: &BTreeSet<String>) {
  let total_count = all_features.len();
  let referenced_count = referenced_features.len();
  let unreferenced_features: Vec<_> = all_features.difference(referenced_features).cloned().collect();

  println!();
  println!("Feature Statistics:");
  println!("  Total Features in GSUB/GPOS: {total_count}");
  println!("  Referenced in Script/Language Systems: {referenced_count}");
  println!("  Unreferenced Features: {unreferenced_features:?}");
}

/// 言語システムに関連付けられた Feature のタグ一覧を取得
///
/// 指定された言語システム（LangSys）に関連付けられた Feature のタグ一覧を取得し、
/// 参照追跡用のセットに記録します。Character Variant 機能の場合は、
/// パラメータ数が2以上であれば追加情報を表示します。
///
/// # 引数
///
/// * `lang_sys` - 対象の言語システム
/// * `features` - Feature リスト
/// * `referenced_features` - 参照された Feature を記録するセット（更新される）
///
/// # 戻り値
///
/// Feature タグの文字列ベクタを返します。
///
/// # エラー
///
/// Feature の取得に失敗した場合にエラーを返します。
fn get_language_features(
  lang_sys: &LangSys,
  features: &FeatureList,
  referenced_features: &mut BTreeSet<String>,
) -> Result<Vec<String>, ScriptLangsError> {
  let mut feature_tags = Vec::new();
  for feature_index in lang_sys.feature_indices() {
    let feature = features.get(feature_index.get()).map_err(|source| ScriptLangsError::FeatureError {
      index: feature_index.get(),
      source,
    })?;
    let mut feature_tag = feature.tag.to_string();
    referenced_features.insert(feature_tag.clone());

    // Feature パラメータが存在する場合の追加情報を取得
    if let Some(params) = feature.feature_params() {
      let feature_params = params.map_err(|source| ScriptLangsError::FeatureParamsError {
        feature_tag: feature_tag.clone(),
        source,
      })?;
      match feature_params {
        // Stylistic Set: 現状追加情報なし
        // Size: 現状追加情報なし
        FeatureParams::StylisticSet(_) | FeatureParams::Size(_) => {},
        // Character Variant: パラメータ数が複数ある場合は表示
        FeatureParams::CharacterVariant(character_variant) => {
          let param_count = character_variant.num_named_parameters();
          if param_count > 1 {
            feature_tag = format!("{feature_tag} (params: {param_count})");
          }
        },
      }
    }
    feature_tags.push(feature_tag);
  }
  return Ok(feature_tags);
}
