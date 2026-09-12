//! OpenType の Script/Language System と Feature の対応を表示するサブコマンド

use std::{collections::BTreeSet, fs, io::Write, path::Path};

use miette::Diagnostic;
use read_fonts::{
  FontRef, ReadError, TableProvider,
  tables::layout::{FeatureList, FeatureParams, LangSys, ScriptList, ScriptRecord},
};
use thiserror::Error;
use tracing::info;

use crate::subcommand::listing;

/// フォントの Script/Language System 解析エラー。
#[derive(Error, Debug, Diagnostic)]
enum ScriptLangsError {
  /// フォントファイルの読み込みエラー
  #[error("フォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(cli::script_langs::read_file), help("フォントファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// 指定インデックスのフォント解析エラー
  #[error("インデックス {font_index} のフォント解析に失敗しました: {path}")]
  #[diagnostic(
    code(cli::script_langs::font_parse_error),
    help(
      "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は別のインデックスを試してください。"
    )
  )]
  FontParse {
    /// ファイルパス
    path: String,
    /// フォント インデックス（TTC の場合）
    font_index: u32,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GSUB テーブルの取得エラー
  #[error("GSUB テーブルが見つからないか、無効です: {path}")]
  #[diagnostic(
    code(cli::script_langs::gsub_error),
    help("このフォントには GSUB（グリフ置換）テーブルが含まれていない可能性があります。")
  )]
  Gsub {
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GPOS テーブルの取得エラー
  #[error("GPOS テーブルが見つからないか、無効です: {path}")]
  #[diagnostic(
    code(cli::script_langs::gpos_error),
    help("このフォントには GPOS（グリフ位置調整）テーブルが含まれていない可能性があります。")
  )]
  Gpos {
    /// ファイルパス
    path: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature リストの取得エラー
  #[error("{table_name} テーブルから Feature リストの取得に失敗しました: {path}")]
  #[diagnostic(
    code(cli::script_langs::feature_list_error),
    help("{table_name} テーブルの構造が破損している可能性があります。フォントファイルを検証してください。")
  )]
  FeatureList {
    /// ファイルパス
    path: String,
    /// テーブル名（"GSUB" または "GPOS"）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Script リストの取得エラー
  #[error("{table_name} テーブルから Script リストの取得に失敗しました: {path}")]
  #[diagnostic(
    code(cli::script_langs::script_list_error),
    help("{table_name} テーブルの構造が破損している可能性があります。フォントファイルを検証してください。")
  )]
  ScriptList {
    /// ファイルパス
    path: String,
    /// テーブル名（"GSUB" または "GPOS"）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature の取得エラー
  #[error("インデックス {index} の Feature の取得に失敗しました: {path}")]
  #[diagnostic(
    code(cli::script_langs::feature_error),
    help("Feature テーブルエントリが無効である可能性があります。Feature リストのインデックスが範囲外かもしれません。")
  )]
  Feature {
    /// ファイルパス
    path: String,
    /// Feature インデックス
    index: u16,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature Parameters の取得エラー
  #[error("Feature '{feature_tag}' のパラメータの取得に失敗しました: {path}")]
  #[diagnostic(
    code(cli::script_langs::feature_params_error),
    help("Feature パラメータ構造が破損している可能性があります。")
  )]
  FeatureParams {
    /// ファイルパス
    path: String,
    /// Feature タグ
    feature_tag: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
}

/// 指定フォントの Script/Language System と Feature の対応の一覧を `out` へ書く。
///
/// # Errors
///
/// ファイルの読み込み、フォントや GSUB/GPOS 内の各テーブルの解析、一覧の書き込み（受け手の終了を除く）に
/// 失敗した場合にエラーを返す。
pub(crate) fn script_langs(file_path: &Path, font_index: u32, out: &mut impl Write) -> miette::Result<()> {
  let font_data = fs::read(file_path).map_err(|source| {
    return ScriptLangsError::ReadFile {
      path: file_path.display().to_string(),
      source,
    };
  })?;
  info!(font_path = %file_path.display(), font_index, "スクリプト・言語を調べるフォントファイルを読込");

  let lines = listing_lines(&font_data, font_index, file_path)?;
  listing::emit(&lines, out)?;
  return Ok(());
}

/// GSUB・GPOS の順に Script/Language System の一覧を組み立て、末尾に Feature の統計を足す。
///
/// # Errors
///
/// フォント、GSUB/GPOS 内の各テーブルの解析に失敗した場合にエラーを返す。
fn listing_lines(font_data: &[u8], font_index: u32, file_path: &Path) -> Result<Vec<String>, ScriptLangsError> {
  let path = || return file_path.display().to_string();
  let font_ref = FontRef::from_index(font_data, font_index).map_err(|source| {
    return ScriptLangsError::FontParse {
      path: path(),
      font_index,
      source,
    };
  })?;
  let mut lines = Vec::new();
  let mut referenced_features = BTreeSet::new();

  let gsub = font_ref.gsub().map_err(|source| {
    return ScriptLangsError::Gsub {
      path: path(),
      source,
    };
  })?;
  let gsub_features = layout_table_lines(
    "GSUB",
    gsub.feature_list(),
    gsub.script_list(),
    file_path,
    &mut referenced_features,
    &mut lines,
  )?;

  let gpos = font_ref.gpos().map_err(|source| {
    return ScriptLangsError::Gpos {
      path: path(),
      source,
    };
  })?;
  let gpos_features = layout_table_lines(
    "GPOS",
    gpos.feature_list(),
    gpos.script_list(),
    file_path,
    &mut referenced_features,
    &mut lines,
  )?;

  let all_features = collect_all_features(&gsub_features, &gpos_features);
  lines.extend(feature_statistics_lines(&all_features, &referenced_features));
  return Ok(lines);
}

/// Layout テーブル 1 つぶんの行を `lines` へ足し、統計用の `FeatureList` を返す。
///
/// # Errors
///
/// Feature リスト、Script リスト、Feature の取得に失敗した場合にエラーを返す。
fn layout_table_lines<'a>(
  table_name: &'static str,
  feature_list: Result<FeatureList<'a>, ReadError>,
  script_list: Result<ScriptList<'a>, ReadError>,
  file_path: &Path,
  referenced_features: &mut BTreeSet<String>,
  lines: &mut Vec<String>,
) -> Result<FeatureList<'a>, ScriptLangsError> {
  let path = || return file_path.display().to_string();
  lines.push(format!("{table_name} Table:"));

  let feature_list = feature_list.map_err(|source| {
    return ScriptLangsError::FeatureList {
      path: path(),
      table_name,
      source,
    };
  })?;
  let script_list = script_list.map_err(|source| {
    return ScriptLangsError::ScriptList {
      path: path(),
      table_name,
      source,
    };
  })?;

  for script_record in script_list.script_records() {
    lines.extend(script_lines(*script_record, &script_list, &feature_list, file_path, referenced_features)?);
  }
  return Ok(feature_list);
}

/// 1 つの `ScriptRecord` について表示する行を組み立てる。
///
/// `Script` サブテーブルや `LangSys` を読めなかったレコードは、読めなかった旨のマーカー行に
/// 置き換えて走査を続ける。黙って飛ばすと「その script / 言語が存在しない」場合と区別が付かず、
/// フォント警告が案内する確認手段としても成立しないため（#432）。レコード同士は独立なので、
/// 1 件の破損で残りのダンプまで失わせない。
///
/// # Errors
///
/// Feature または Feature Parameters の取得に失敗した場合にエラーを返す（`FeatureList` の
/// 索引ずれはレコード単位で閉じないので、こちらは打ち切る）。
fn script_lines(
  script_record: ScriptRecord,
  scripts: &ScriptList<'_>,
  features: &FeatureList<'_>,
  file_path: &Path,
  referenced_features: &mut BTreeSet<String>,
) -> Result<Vec<String>, ScriptLangsError> {
  let script_tag = script_record.script_tag().to_string();
  let mut lines = vec![format!("  Script: {script_tag}")];

  // `ScriptRecord::script` は `Script` サブテーブルの Offset16 をフォントバイト列から
  // 解決するだけ（read-fonts の `Offset16::resolve`）。オフセットが 0 なら
  // `ReadError::NullOffset`、範囲外・切り詰めなら `ReadError::OutOfBounds` を返すので、
  // 破損フォントで到達する
  let subtable = match script_record.script(scripts.offset_data()) {
    Ok(subtable) => subtable,
    Err(source) => {
      lines.push(format!("    (Script サブテーブルの読み取りに失敗しました: {source})"));
      return Ok(lines);
    },
  };

  // こちらは nullable な Offset16 なので、NULL は `None` に畳まれている。
  // `Some(Err(_))` は「NULL ではないが解決できない」＝破損フォントの側
  if let Some(default_lang_sys) = subtable.default_lang_sys() {
    match default_lang_sys {
      Ok(lang_sys) => {
        let feature_tags = get_language_features(&lang_sys, features, file_path, referenced_features)?;
        lines.push(format!("    Default Language System: {feature_tags:?}"));
      },
      Err(source) => {
        lines.push(format!("    (既定 Language System の読み取りに失敗しました: {source})"));
      },
    }
  }

  for lang_record in subtable.lang_sys_records() {
    let lang_tag = lang_record.lang_sys_tag().to_string();
    // `LangSysRecord::lang_sys` も nullable でない Offset16 の解決なので、`script` と同じく
    // 破損フォントで失敗しうる。タグはレコード本体にあるので、読めなくてもどの言語かは出せる
    match lang_record.lang_sys(subtable.offset_data()) {
      Ok(lang_sys) => {
        let feature_tags = get_language_features(&lang_sys, features, file_path, referenced_features)?;
        lines.push(format!("    {lang_tag}: {feature_tags:?}"));
      },
      Err(source) => {
        lines.push(format!("    {lang_tag}: (Language System の読み取りに失敗しました: {source})"));
      },
    }
  }

  return Ok(lines);
}

/// GSUB と GPOS の Feature タグを重複なく統合する。
fn collect_all_features(gsub_features: &FeatureList<'_>, gpos_features: &FeatureList<'_>) -> BTreeSet<String> {
  let mut all_features = BTreeSet::new();
  insert_feature_tags(gsub_features, &mut all_features);
  insert_feature_tags(gpos_features, &mut all_features);
  return all_features;
}

/// `FeatureList` のタグを集合へ追加する。
fn insert_feature_tags(feature_list: &FeatureList<'_>, all_features: &mut BTreeSet<String>) {
  for record in feature_list.feature_records() {
    all_features.insert(record.feature_tag().to_string());
  }
}

/// Feature の総数・参照数・未参照一覧の行（先頭に区切りの空行）を返す。
fn feature_statistics_lines(all_features: &BTreeSet<String>, referenced_features: &BTreeSet<String>) -> Vec<String> {
  let total_count = all_features.len();
  let referenced_count = referenced_features.len();
  let unreferenced_features: Vec<_> = all_features.difference(referenced_features).cloned().collect();

  return vec![
    String::new(),
    "Feature Statistics:".to_owned(),
    format!("  Total Features in GSUB/GPOS: {total_count}"),
    format!("  Referenced in Script/Language Systems: {referenced_count}"),
    format!("  Unreferenced Features: {unreferenced_features:?}"),
  ];
}

/// Language System が参照する Feature タグを返し、参照済み集合へ記録する。
///
/// Character Variant の名前付きパラメータが複数あれば、個数をタグへ付記する。
///
/// # Errors
///
/// Feature または Feature Parameters の取得に失敗した場合にエラーを返す。
fn get_language_features(
  lang_sys: &LangSys<'_>,
  features: &FeatureList<'_>,
  file_path: &Path,
  referenced_features: &mut BTreeSet<String>,
) -> Result<Vec<String>, ScriptLangsError> {
  let mut feature_tags = Vec::new();

  for feature_index in lang_sys.feature_indices() {
    let feature = features.get(feature_index.get()).map_err(|source| {
      return ScriptLangsError::Feature {
        path: file_path.display().to_string(),
        index: feature_index.get(),
        source,
      };
    })?;

    let mut feature_tag = feature.tag.to_string();
    referenced_features.insert(feature_tag.clone());

    if let Some(params) = feature.feature_params() {
      let feature_params = params.map_err(|source| {
        return ScriptLangsError::FeatureParams {
          path: file_path.display().to_string(),
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

#[cfg(test)]
mod tests {
  use read_fonts::{FontData, FontRead};

  use super::*;

  /// 正常な `Script` テーブルを指す `scriptOffset`（`ScriptList` 先頭 + scriptCount + レコード 1 件）。
  const SCRIPT_OFFSET: u16 = 8;

  /// `Script` テーブル先頭からの Offset16 として範囲外になる値。
  const OUT_OF_BOUNDS_OFFSET: u16 = 0xffff;

  /// feature index 0 だけを参照する `LangSys` テーブルのバイト列（8 バイト固定）。
  fn lang_sys_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u16.to_be_bytes()); // lookupOrderOffset（NULL）
    bytes.extend_from_slice(&0xffffu16.to_be_bytes()); // requiredFeatureIndex（無し）
    bytes.extend_from_slice(&1u16.to_be_bytes()); // featureIndexCount
    bytes.extend_from_slice(&0u16.to_be_bytes()); // featureIndices[0]
    return bytes;
  }

  /// タグ `liga` の Feature 1 件だけを持つ `FeatureList` のバイト列。
  fn feature_list_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_be_bytes()); // featureCount
    bytes.extend_from_slice(b"liga"); // featureTag
    bytes.extend_from_slice(&8u16.to_be_bytes()); // featureOffset
    bytes.extend_from_slice(&0u16.to_be_bytes()); // Feature.featureParamsOffset（NULL）
    bytes.extend_from_slice(&0u16.to_be_bytes()); // Feature.lookupIndexCount
    return bytes;
  }

  /// script タグ `latn` 1 件だけを持つ `ScriptList` のバイト列を組む。
  ///
  /// `script_offset` は `ScriptList` 先頭からの Offset16 で、`SCRIPT_OFFSET` を渡すと
  /// 下で組む `Script` テーブルを指す。`default_lang_sys` が真なら既定 Language System を置く。
  /// `lang_records` の第 2 要素が `None` なら正常な `LangSys` を指し、`Some(v)` は
  /// その値をそのまま `langSysOffset` へ書く。
  fn script_list_bytes(script_offset: u16, default_lang_sys: bool, lang_records: &[(&str, Option<u16>)]) -> Vec<u8> {
    let record_count = u16::try_from(lang_records.len()).expect("テストのレコード数は u16 に収まる");
    // `Script` テーブル先頭から見た最初の `LangSys` 本体の位置（ヘッダ 4 + レコード 6 * N の直後）
    let mut body_offset = 4 + 6 * record_count;
    let mut bodies = Vec::new();

    let mut script = Vec::new();
    if default_lang_sys {
      script.extend_from_slice(&body_offset.to_be_bytes()); // defaultLangSysOffset
      body_offset += 8;
      bodies.extend_from_slice(&lang_sys_bytes());
    } else {
      script.extend_from_slice(&0u16.to_be_bytes()); // defaultLangSysOffset（NULL）
    }
    script.extend_from_slice(&record_count.to_be_bytes()); // langSysCount
    for (lang_tag, offset_override) in lang_records {
      script.extend_from_slice(lang_tag.as_bytes()); // langSysTag
      if let Some(offset) = offset_override {
        script.extend_from_slice(&offset.to_be_bytes());
      } else {
        script.extend_from_slice(&body_offset.to_be_bytes());
        body_offset += 8;
        bodies.extend_from_slice(&lang_sys_bytes());
      }
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    bytes.extend_from_slice(b"latn"); // scriptTag
    bytes.extend_from_slice(&script_offset.to_be_bytes()); // scriptOffset
    bytes.extend_from_slice(&script);
    bytes.extend_from_slice(&bodies);
    return bytes;
  }

  /// 組んだバイト列から `script_lines` を呼び、行と参照済み Feature を返す。
  fn run_script_lines(script_list: &[u8]) -> (Vec<String>, BTreeSet<String>) {
    let scripts = ScriptList::read(FontData::new(script_list)).expect("ScriptList 自体は読めるはず");
    let feature_bytes = feature_list_bytes();
    let features = FeatureList::read(FontData::new(&feature_bytes)).expect("FeatureList は読めるはず");
    let mut referenced_features = BTreeSet::new();
    let record = scripts.script_records().first().expect("script レコードを 1 件置いている");
    let lines = script_lines(*record, &scripts, &features, Path::new("test.otf"), &mut referenced_features)
      .expect("Feature の取得は成功する");
    return (lines, referenced_features);
  }

  #[test]
  fn unreadable_script_subtable_is_reported_as_a_marker_line() {
    // Arrange
    let bytes = script_list_bytes(OUT_OF_BOUNDS_OFFSET, false, &[]);

    // Act
    let (lines, referenced_features) = run_script_lines(&bytes);

    // Assert
    assert_eq!(lines.len(), 2, "script タグの行と失敗を伝えるマーカー行の 2 行が出るはず");
    assert_eq!(lines[0], "  Script: latn");
    assert!(
      lines[1].starts_with("    (Script サブテーブルの読み取りに失敗しました:"),
      "黙って飛ばさず読み取り失敗を出力に残すはず: {}",
      lines[1]
    );
    assert!(referenced_features.is_empty(), "読めなかった script は Feature を参照しない");
  }

  #[test]
  fn null_script_offset_is_reported_as_a_marker_line() {
    // Arrange
    let bytes = script_list_bytes(0, false, &[]);

    // Act
    let (lines, _) = run_script_lines(&bytes);

    // Assert
    assert_eq!(lines.len(), 2, "NULL オフセットも読み取り失敗として 1 行に出るはず");
    assert!(
      lines[1].starts_with("    (Script サブテーブルの読み取りに失敗しました:"),
      "NULL オフセットも黙って飛ばさないはず: {}",
      lines[1]
    );
  }

  #[test]
  fn unreadable_default_lang_sys_is_reported_as_a_marker_line() {
    // Arrange
    let mut bytes = script_list_bytes(SCRIPT_OFFSET, true, &[]);
    let default_offset_at = usize::from(SCRIPT_OFFSET);
    bytes[default_offset_at..default_offset_at + 2].copy_from_slice(&OUT_OF_BOUNDS_OFFSET.to_be_bytes());

    // Act
    let (lines, referenced_features) = run_script_lines(&bytes);

    // Assert
    assert_eq!(lines.len(), 2, "既定 Language System の失敗も 1 行として残るはず");
    assert!(
      lines[1].starts_with("    (既定 Language System の読み取りに失敗しました:"),
      "打ち切らずマーカー行にするはず: {}",
      lines[1]
    );
    assert!(referenced_features.is_empty(), "読めなかった LangSys は Feature を参照しない");
  }

  #[test]
  fn unreadable_lang_sys_is_reported_but_other_languages_are_listed() {
    // Arrange
    let bytes = script_list_bytes(SCRIPT_OFFSET, false, &[("JAN ", None), ("TRK ", Some(OUT_OF_BOUNDS_OFFSET))]);

    // Act
    let (lines, referenced_features) = run_script_lines(&bytes);

    // Assert
    assert_eq!(lines.len(), 3, "script タグの行と言語 2 件の行が出るはず");
    assert_eq!(lines[1], "    JAN : [\"liga\"]", "読めた言語は従来どおり一覧に出る");
    assert!(
      lines[2].starts_with("    TRK : (Language System の読み取りに失敗しました:"),
      "読めなかった言語はタグ付きのマーカー行になるはず: {}",
      lines[2]
    );
    assert_eq!(referenced_features, BTreeSet::from(["liga".to_owned()]));
  }

  #[test]
  fn readable_script_lists_its_language_systems() {
    // Arrange
    let bytes = script_list_bytes(SCRIPT_OFFSET, true, &[("JAN ", None)]);

    // Act
    let (lines, referenced_features) = run_script_lines(&bytes);

    // Assert
    assert_eq!(
      lines,
      vec![
        "  Script: latn".to_owned(),
        "    Default Language System: [\"liga\"]".to_owned(),
        "    JAN : [\"liga\"]".to_owned(),
      ]
    );
    assert_eq!(referenced_features, BTreeSet::from(["liga".to_owned()]));
  }
}
