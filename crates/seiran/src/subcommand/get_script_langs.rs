use std::{fs, path::Path};

use ttf_parser::{
  Face,
  opentype_layout::{FeatureList, LanguageSystem, LayoutTable},
};

/// フォントファイルのscriptとlanguageシステムを表示
///
/// 指定されたフォントファイルから、利用可能なscriptとlanguageシステムを
/// 取得して標準出力に表示します。
///
/// # 引数
///
/// * `file_path` - フォントファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイルの読み込みまたはフォント解析に失敗した場合にエラーを返します。
pub(crate) fn get_script_langs<P: AsRef<Path>>(file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let font_data = fs::read(file_path)?;
  let face = Face::parse(&font_data, 0)?;
  let tables = face.tables();

  match tables.gsub {
    Some(gsub) => {
      println!("GSUB Table:");
      print_layouttable(gsub);
    },
    None => {
      println!("No GSUB table found in the font.");
    },
  }

  match tables.gpos {
    Some(gpos) => {
      println!("GPOS Table:");
      print_layouttable(gpos);
    },
    None => {
      println!("No GPOS table found in the font.");
    },
  }

  return Ok(());
}

/// レイアウトテーブルの内容を標準出力に表示
///
/// GSUBまたはGPOSテーブルの内容を解析し、スクリプト、言語システム、
/// フィーチャー情報を標準出力に表示します。
///
/// # 引数
///
/// * `layout_table` - 表示するレイアウトテーブル
fn print_layouttable(layout_table: LayoutTable) {
  let scripts = layout_table.scripts;
  let features = layout_table.features;

  for script in scripts {
    let script_tag = script.tag.to_string();
    println!("Script: {}", script_tag);
    let default_lang = script.default_language;
    if let Some(default_lang) = default_lang {
      let (lang_tag, feature_tags) = get_language_system(&default_lang, &features);
      println!("  Default Language System: {}, Features: {:?}", lang_tag, feature_tags);
    }
    for lang_sys in script.languages {
      let (lang_tag, feature_tags) = get_language_system(&lang_sys, &features);
      println!("  Language System: {}, Features: {:?}", lang_tag, feature_tags);
    }
  }
}

/// 言語システムの情報を取得
///
/// 指定された言語システムから、言語タグとサポートするフィーチャータグのリストを取得します。
///
/// # 引数
///
/// * `lang_sys` - 言語システム
/// * `features` - フィーチャーリスト
///
/// # 戻り値
///
/// 言語タグ（String）とフィーチャータグのリスト（Vec<String>）のタプルを返します。
fn get_language_system(lang_sys: &LanguageSystem, features: &FeatureList) -> (String, Vec<String>) {
  let lang_tag = lang_sys.tag.to_string();
  let feature_indices = lang_sys.feature_indices;
  let feature_tags = feature_indices
    .into_iter()
    .filter_map(|index| features.get(index))
    .map(|feature| feature.tag.to_string())
    .collect::<Vec<_>>();
  return (lang_tag, feature_tags);
}
