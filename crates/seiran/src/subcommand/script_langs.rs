//! OpenType の Script/Language System と Feature の対応を表示するサブコマンド

use std::{collections::BTreeSet, fs, path::Path};

use miette::Diagnostic;
use read_fonts::{
  FontRef, ReadError, TableProvider,
  tables::layout::{FeatureList, FeatureParams, LangSys, ScriptList},
};
use thiserror::Error;
use tracing::info;

/// フォントの Script/Language System 解析エラー。
#[derive(Error, Debug, Diagnostic)]
#[allow(clippy::enum_variant_names)]
enum ScriptLangsError {
  /// フォントファイルの読み込みエラー
  #[error("フォントファイルの読み込みに失敗しました: {0}")]
  #[diagnostic(
    code(cli::script_langs::io_error),
    help("ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  IoError(#[from] std::io::Error),

  /// 指定インデックスのフォント解析エラー
  #[error("インデックス {font_index} のフォント解析に失敗しました")]
  #[diagnostic(
    code(cli::script_langs::font_parse_error),
    help(
      "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は別のインデックスを試してください。"
    )
  )]
  FontParseError {
    /// フォント インデックス（TTC の場合）
    font_index: u32,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GSUB テーブルの取得エラー
  #[error("GSUB テーブルが見つからないか、無効です")]
  #[diagnostic(
    code(cli::script_langs::gsub_error),
    help("このフォントには GSUB（グリフ置換）テーブルが含まれていない可能性があります。")
  )]
  GsubError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GPOS テーブルの取得エラー
  #[error("GPOS テーブルが見つからないか、無効です")]
  #[diagnostic(
    code(cli::script_langs::gpos_error),
    help("このフォントには GPOS（グリフ位置調整）テーブルが含まれていない可能性があります。")
  )]
  GposError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature リストの取得エラー
  #[error("{table_name} テーブルから Feature リストの取得に失敗しました")]
  #[diagnostic(
    code(cli::script_langs::feature_list_error),
    help("{table_name} テーブルの構造が破損している可能性があります。フォントファイルを検証してください。")
  )]
  FeatureListError {
    /// テーブル名（"GSUB" または "GPOS"）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Script リストの取得エラー
  #[error("{table_name} テーブルから Script リストの取得に失敗しました")]
  #[diagnostic(
    code(cli::script_langs::script_list_error),
    help("{table_name} テーブルの構造が破損している可能性があります。フォントファイルを検証してください。")
  )]
  ScriptListError {
    /// テーブル名（"GSUB" または "GPOS"）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Language System の取得エラー
  #[error("Script '{script_tag}' のインデックス {index} の Language System の取得に失敗しました")]
  #[diagnostic(
    code(cli::script_langs::lang_sys_error),
    help("Language System エントリが無効であるか、Script リスト構造が破損している可能性があります。")
  )]
  LangSysError {
    /// Language System インデックス
    index: u16,
    /// Script タグ
    script_tag: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature の取得エラー
  #[error("インデックス {index} の Feature の取得に失敗しました")]
  #[diagnostic(
    code(cli::script_langs::feature_error),
    help("Feature テーブルエントリが無効である可能性があります。Feature リストのインデックスが範囲外かもしれません。")
  )]
  FeatureError {
    /// Feature インデックス
    index: u16,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature Parameters の取得エラー
  #[error("Feature '{feature_tag}' のパラメータの取得に失敗しました")]
  #[diagnostic(
    code(cli::script_langs::feature_params_error),
    help("Feature パラメータ構造が破損している可能性があります。")
  )]
  FeatureParamsError {
    /// Feature タグ
    feature_tag: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
}

/// 指定フォントの Script/Language System と Feature の対応を標準出力へ表示する。
///
/// # Errors
///
/// ファイル、フォント、GSUB/GPOS 内の各テーブルの解析に失敗した場合にエラーを返す。
pub(crate) fn script_langs(file_path: &Path, font_index: u32) -> miette::Result<()> {
  info!(font_path = %file_path.display(), font_index, "フォントファイルを読み込みます");

  let mut referenced_features = BTreeSet::new();

  let font_data = fs::read(file_path).map_err(ScriptLangsError::from)?;
  let font_ref = FontRef::from_index(&font_data, font_index)
    .map_err(|source| return ScriptLangsError::FontParseError { font_index, source })?;

  let gsub = font_ref.gsub().map_err(|source| return ScriptLangsError::GsubError { source })?;
  let gsub_features = process_layout_table("GSUB", gsub.feature_list(), gsub.script_list(), &mut referenced_features)?;

  let gpos = font_ref.gpos().map_err(|source| return ScriptLangsError::GposError { source })?;
  let gpos_features = process_layout_table("GPOS", gpos.feature_list(), gpos.script_list(), &mut referenced_features)?;

  let all_features = collect_all_features(&gsub_features, &gpos_features);
  print_feature_statistics(&all_features, &referenced_features);

  return Ok(());
}

/// Layout テーブルを走査して表示し、統計用の `FeatureList` を返す。
///
/// # Errors
///
/// Feature、Script、Language System の取得に失敗した場合にエラーを返す。
fn process_layout_table<'a>(
  table_name: &'static str,
  feature_list: Result<FeatureList<'a>, ReadError>,
  script_list: Result<ScriptList<'a>, ReadError>,
  referenced_features: &mut BTreeSet<String>,
) -> Result<FeatureList<'a>, ScriptLangsError> {
  println!("{table_name} Table:");

  let feature_list = feature_list.map_err(|source| return ScriptLangsError::FeatureListError { table_name, source })?;
  let script_list = script_list.map_err(|source| return ScriptLangsError::ScriptListError { table_name, source })?;

  print_scripts(&script_list, &feature_list, referenced_features)?;

  return Ok(feature_list);
}

/// Script ごとの Language System と Feature を表示する。
///
/// # Errors
///
/// Language System または Feature の取得に失敗した場合にエラーを返す。
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
        let default_lang = default_lang_sys.map_err(|source| {
          return ScriptLangsError::LangSysError {
            index: 0,
            script_tag: script_tag.clone(),
            source,
          };
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

/// GSUB と GPOS の Feature タグを重複なく統合する。
fn collect_all_features(
  gsub_features: &FeatureList,
  gpos_features: &FeatureList,
) -> std::collections::BTreeSet<std::string::String> {
  let mut all_features = BTreeSet::new();
  insert_feature_tags(gsub_features, &mut all_features);
  insert_feature_tags(gpos_features, &mut all_features);
  return all_features;
}

/// `FeatureList` のタグを集合へ追加する。
fn insert_feature_tags(feature_list: &FeatureList, all_features: &mut BTreeSet<String>) {
  for record in feature_list.feature_records() {
    all_features.insert(record.feature_tag().to_string());
  }
}

/// Feature の総数・参照数・未参照一覧を表示する。
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

/// Language System が参照する Feature タグを返し、参照済み集合へ記録する。
///
/// Character Variant の名前付きパラメータが複数あれば、個数をタグへ付記する。
///
/// # Errors
///
/// Feature または Feature Parameters の取得に失敗した場合にエラーを返す。
fn get_language_features(
  lang_sys: &LangSys,
  features: &FeatureList,
  referenced_features: &mut BTreeSet<String>,
) -> Result<Vec<String>, ScriptLangsError> {
  let mut feature_tags = Vec::new();

  for feature_index in lang_sys.feature_indices() {
    let feature = features.get(feature_index.get()).map_err(|source| {
      return ScriptLangsError::FeatureError {
        index: feature_index.get(),
        source,
      };
    })?;

    let mut feature_tag = feature.tag.to_string();
    referenced_features.insert(feature_tag.clone());

    if let Some(params) = feature.feature_params() {
      let feature_params = params.map_err(|source| {
        return ScriptLangsError::FeatureParamsError {
          feature_tag: feature_tag.clone(),
          source,
        };
      })?;
      match feature_params {
        FeatureParams::StylisticSet(_) | FeatureParams::Size(_) => {},
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
