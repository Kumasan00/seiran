//! フォント対応の OpenType Script/Language タグ表示サブコマンド
//!
//! フォントが対応する言語・文字体系（Script）と言語システム（Language）の組み合わせを確認します。
//! これは OpenType テキストシェイピングの中核を成す情報です。
//!
//! ## OpenType レイアウトテーブルとは
//!
//! 複雑な文字表現（合字、文字変形、位置調整など）を行うための OpenType 機能は、
//! 以下のテーブルで定義されます：
//!
//! - **GSUB (Glyph Substitution)** - グリフの置換（例：f + i → fi 合字）
//! - **GPOS (Glyph Positioning)** - グリフの位置調整（例：カーニング）
//!
//! ## Script と Language System
//!
//! 各レイアウトテーブル（GSUB/GPOS）内で、特定の言語・文字体系に対応する機能は
//! Script（文字体系）と Language System（言語）の階層構造で管理されます。
//!
//! | 用語             | 説明                                      | 例                                |
//! | -------------- | ---------------------------------------- | -------------------------------- |
//! | Script         | 文字体系（4 文字タグ）                      | `latn`（Latin）、`arab`（Arabic） |
//! | Language System | 言語・地域（3-4 文字タグ）                 | `JAN`（日本語）、`KOR`（韓国語） |
//! | Feature        | 具体的なシェイピング機能（4 文字タグ）     | `liga`（合字）、`kern`（カーニング） |
//!
//! ## Feature の参照状況
//!
//! レイアウトテーブルに定義された Feature すべてが、Script/Language System から参照されるわけではありません。
//! このコマンドは以下を表示します：
//! - 各 Script/Language で参照されている Feature
//! - 参照されていない「孤立した」Feature の一覧
//!
//! ## 使用例
//!
//! ```bash
//! # シングル TTF フォントの Script/Language 確認
//! seiran script-langs fonts/NotoSerifJP.otf
//!
//! # TTC ファイル内の特定フォント（インデックス 1）を確認
//! seiran script-langs fonts/SourceHanCodeJP.ttc --font-index 1
//! ```

use std::{collections::BTreeSet, fs, path::Path};

use miette::Diagnostic;
use read_fonts::{
  FontRef, ReadError, TableProvider,
  tables::layout::{FeatureList, FeatureParams, LangSys, ScriptList},
};
use thiserror::Error;
use tracing::info;

/// フォント Script/Language システム解析時のエラー型
///
/// OpenType レイアウトテーブル（GSUB/GPOS）またはその内部構造の解析に失敗した場合のエラーです。
/// 各エラーは詳細なコンテキスト情報と解決方法を含みます。
#[derive(Error, Debug, Diagnostic)]
#[allow(clippy::enum_variant_names)]
enum ScriptLangsError {
  /// ファイル読み込みエラー
  ///
  /// フォントファイルをメモリに読み込めません。
  /// ファイルが存在しない、アクセス権限がない、ディスクエラーなどが考えられます。
  #[error("フォントファイルの読み込みに失敗しました: {0}")]
  #[diagnostic(code(script_langs::io_error), help("ファイルが存在し、読み取り権限があることを確認してください。"))]
  IoError(#[from] std::io::Error),

  /// フォント解析エラー
  ///
  /// フォントファイルは読み込めましたが、指定インデックスのフォントを解析できません。
  /// TTC ファイルの場合、範囲外のインデックスを指定した可能性があります。
  #[error("インデックス {font_index} のフォント解析に失敗しました")]
  #[diagnostic(
    code(script_langs::font_parse_error),
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

  /// GSUB テーブル欠落エラー
  ///
  /// フォントが GSUB（Glyph Substitution）テーブルを含みません。
  /// グリフ置換機能（合字など）をサポートしないフォントの可能性があります。
  #[error("GSUB テーブルが見つからないか、無効です")]
  #[diagnostic(
    code(script_langs::gsub_error),
    help("このフォントには GSUB（グリフ置換）テーブルが含まれていない可能性があります。")
  )]
  GsubError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// GPOS テーブル欠落エラー
  ///
  /// フォントが GPOS（Glyph Positioning）テーブルを含みません。
  /// グリフ位置調整機能（カーニングなど）をサポートしないフォントの可能性があります。
  #[error("GPOS テーブルが見つからないか、無効です")]
  #[diagnostic(
    code(script_langs::gpos_error),
    help("このフォントには GPOS（グリフ位置調整）テーブルが含まれていない可能性があります。")
  )]
  GposError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature リスト取得エラー
  ///
  /// GSUB/GPOS テーブル内の Feature リスト構造が破損しているか、アクセスできません。
  #[error("{table_name} テーブルから Feature リストの取得に失敗しました")]
  #[diagnostic(
    code(script_langs::feature_list_error),
    help("{table_name} テーブルの構造が破損している可能性があります。フォントファイルを検証してください。")
  )]
  FeatureListError {
    /// テーブル名（"GSUB" または "GPOS"）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Script リスト取得エラー
  ///
  /// GSUB/GPOS テーブル内の Script リスト構造が破損しているか、アクセスできません。
  #[error("{table_name} テーブルから Script リストの取得に失敗しました")]
  #[diagnostic(
    code(script_langs::script_list_error),
    help("{table_name} テーブルの構造が破損している可能性があります。フォントファイルを検証してください。")
  )]
  ScriptListError {
    /// テーブル名（"GSUB" または "GPOS"）
    table_name: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Language System 取得エラー
  ///
  /// 特定の Script 内の Language System 構造にアクセスできません。
  #[error("Script '{script_tag}' のインデックス {index} の Language System の取得に失敗しました")]
  #[diagnostic(
    code(script_langs::lang_sys_error),
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

  /// Feature 取得エラー
  ///
  /// Feature リスト内の特定 Feature にアクセスできません。
  /// インデックスが無効または Feature テーブルが破損している可能性があります。
  #[error("インデックス {index} の Feature の取得に失敗しました")]
  #[diagnostic(
    code(script_langs::feature_error),
    help("Feature テーブルエントリが無効である可能性があります。Feature リストのインデックスが範囲外かもしれません。")
  )]
  FeatureError {
    /// Feature インデックス
    index: u16,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature Parameters 取得エラー
  ///
  /// Feature の追加パラメータ情報にアクセスできません。
  /// すべての Feature がパラメータを持つわけではないため、エラーが常に問題になるわけではありません。
  #[error("Feature '{feature_tag}' のパラメータの取得に失敗しました")]
  #[diagnostic(
    code(script_langs::feature_params_error),
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

/// 指定フォントの Script/Language System と Feature の対応状況を標準出力に表示する
///
/// GSUB/GPOS テーブルを解析し、各 Script（文字体系）・Language System（言語）が
/// 参照する OpenType Feature の一覧と、フォント全体の Feature 統計情報を出力します。
///
/// # Arguments
///
/// * `file_path` - フォントファイルへのパス（例：`NotoSerifJP.otf`、`SourceHanCodeJP.ttc`）
/// * `font_index` - TTC/OTC 内のフォントインデックス（通常は `0`）
///
/// # Errors
///
/// - ファイル I/O エラー
/// - 指定インデックスのフォント解析失敗
/// - GSUB/GPOS テーブルが存在しないまたは破損している
/// - Script リスト・Language System・Feature テーブルへのアクセス失敗
///
/// # 出力形式
///
/// ```text
/// GSUB Table:
///   Script: {script_tag}
///     Default Language System: {feature_tags}
///     {lang_tag}: {feature_tags}
/// GPOS Table:
///   Script: {script_tag}
///     Default Language System: {feature_tags}
///     {lang_tag}: {feature_tags}
///
/// Feature Statistics:
///   Total Features in GSUB/GPOS: {count}
///   Referenced in Script/Language Systems: {count}
///   Unreferenced Features: {feature_list}
/// ```
///
/// # Example
///
/// ```ignore
/// seiran script-langs fonts/NotoSerifJP.otf
/// ```
pub fn script_langs(file_path: &Path, font_index: u32) -> miette::Result<()> {
  info!(font_file_path = %file_path.display(), font_index = font_index, "Input font file path and index");

  // Script/Language System から参照された Feature タグを収集する（統計処理で使用）
  let mut referenced_features = BTreeSet::new();

  let font_data = fs::read(file_path).map_err(ScriptLangsError::from)?;
  let font_ref = FontRef::from_index(&font_data, font_index)
    .map_err(|source| ScriptLangsError::FontParseError { font_index, source })?;

  // GSUB（グリフ置換）・GPOS（グリフ位置調整）の各テーブルを処理
  let gsub = font_ref.gsub().map_err(|source| ScriptLangsError::GsubError { source })?;
  let gsub_features = process_layout_table("GSUB", gsub.feature_list(), gsub.script_list(), &mut referenced_features)?;

  let gpos = font_ref.gpos().map_err(|source| ScriptLangsError::GposError { source })?;
  let gpos_features = process_layout_table("GPOS", gpos.feature_list(), gpos.script_list(), &mut referenced_features)?;

  // GSUB/GPOS の全 Feature を統合し、未参照 Feature の統計を表示
  let all_features = collect_all_features(&gsub_features, &gpos_features);
  print_feature_statistics(&all_features, &referenced_features);

  return Ok(());
}

/// Layout テーブル（GSUB/GPOS）の Script/Language System を走査して表示し、`FeatureList` を返す
///
/// `ScriptList` 内の全 Script・Language System を出力し、各 Language System が参照する
/// Feature タグを `referenced_features` に記録します。
/// 呼び出し元が Feature 統計を計算できるよう、`FeatureList` を返します。
///
/// # Arguments
///
/// * `table_name` - テーブル名（`"GSUB"` または `"GPOS"`）。表示用ヘッダーに使用
/// * `feature_list` - Feature タグ取得に使用する `FeatureList`
/// * `script_list` - Script/Language System の列挙に使用する `ScriptList`
/// * `referenced_features` - 参照済み Feature タグを記録するセット（関数内で拡張される）
///
/// # Returns
///
/// 成功時に `FeatureList<'a>` を返します。ライフタイムは元のフォントデータに紐付けられます。
///
/// # Errors
///
/// - **`FeatureListError`** - Feature リスト構造が破損または読み取り不可
/// - **`ScriptListError`** - Script リスト構造が破損または読み取り不可
/// - **`LangSysError`** - Language System へのアクセスに失敗
/// - **`FeatureError`** - Feature タグの取得に失敗
///
/// # 出力例
///
/// ```text
/// GSUB Table:
///   Script: latn
///     Default Language System: ["liga", "dlig", "calt"]
///     JAN: ["liga", "dlig", "calt", "fwid"]
///   Script: arab
///     Default Language System: ["init", "medi", "fina", "isol"]
/// ```
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

/// `ScriptList` を走査して Script・Language System・Feature の対応を標準出力に表示する
///
/// 各 Script タグと、その配下の Default Language System および言語固有 Language System
/// が参照する Feature タグ一覧を出力します。
/// 同時に、参照した Feature タグを `referenced_features` に記録します。
///
/// # Arguments
///
/// * `scripts` - 走査対象の `ScriptList`
/// * `features` - Feature タグ取得に使用する `FeatureList`
/// * `referenced_features` - 参照済み Feature タグを記録するセット（関数内で拡張される）
///
/// # Errors
///
/// - **`LangSysError`** - Language System レコードへのアクセス失敗
/// - **`FeatureError`** - Feature タグ取得に失敗
///
/// # 出力例
///
/// ```text
///   Script: arab
///     Default Language System: ["rlig", "fina"]
///     AFR: ["rlig", "fina", "loclAFR"]
///     URD: ["rlig", "fina", "loclURD"]
///   Script: latn
///     Default Language System: ["liga", "kern"]
///     DEU: ["liga", "kern", "loclDEU"]
/// ```
fn print_scripts(
  scripts: &ScriptList,
  features: &FeatureList,
  referenced_features: &mut BTreeSet<String>,
) -> Result<(), ScriptLangsError> {
  for script_record in scripts.script_records() {
    let script_tag = script_record.script_tag().to_string();
    println!("  Script: {script_tag}");

    if let Ok(subtable) = script_record.script(scripts.offset_data()) {
      // Default Language System: すべての言語に共通して適用される Feature を定義する
      if let Some(default_lang_sys) = subtable.default_lang_sys() {
        let default_lang = default_lang_sys.map_err(|source| ScriptLangsError::LangSysError {
          index: 0,
          script_tag: script_tag.clone(),
          source,
        })?;
        let feature_tags = get_language_features(&default_lang, features, referenced_features)?;
        println!("    Default Language System: {feature_tags:?}");
      }

      // 言語固有の Language System（例："JAN" = 日本語、"KOR" = 韓国語）
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

/// GSUB と GPOS の全 Feature タグを重複排除してひとつのセットに統合する
///
/// 同じ Feature タグが GSUB/GPOS の両方に定義される場合もありますが、
/// `BTreeSet` の特性により自動的に重複が排除されてアルファベット順で返されます。
///
/// # Arguments
///
/// * `gsub_features` - GSUB の `FeatureList`
/// * `gpos_features` - GPOS の `FeatureList`
///
/// # Returns
///
/// GSUB/GPOS 全体の一意な Feature タグセット（アルファベット順）。
/// 例：GSUB に `["liga", "dlig"]`・GPOS に `["kern", "liga"]` があれば `{"dlig", "kern", "liga"}`
fn collect_all_features(
  gsub_features: &FeatureList,
  gpos_features: &FeatureList,
) -> std::collections::BTreeSet<std::string::String> {
  let mut all_features = BTreeSet::new();
  insert_feature_tags(gsub_features, &mut all_features);
  insert_feature_tags(gpos_features, &mut all_features);
  return all_features;
}

/// `FeatureList` 内の全 Feature タグを `all_features` に追加する
///
/// `collect_all_features()` の内部ヘルパー関数です。
///
/// # Arguments
///
/// * `feature_list` - 抽出対象の `FeatureList`（GSUB または GPOS）
/// * `all_features` - タグの追加先セット
fn insert_feature_tags(feature_list: &FeatureList, all_features: &mut BTreeSet<String>) {
  for record in feature_list.feature_records() {
    all_features.insert(record.feature_tag().to_string());
  }
}

/// Feature 統計情報（全数・参照数・未参照一覧）を標準出力に表示する
///
/// フォントに定義された Feature のうち、Script/Language System から参照されていない
/// 「未参照 Feature」を検出・表示します。未参照 Feature はフォント設計の不備や
/// 言語サポートの欠落を示す可能性があります。
///
/// # Arguments
///
/// * `all_features` - GSUB/GPOS の全 Feature タグセット
/// * `referenced_features` - Script/Language System から参照された Feature タグセット
///
/// # 出力形式
///
/// ```text
/// Feature Statistics:
///   Total Features in GSUB/GPOS: 32
///   Referenced in Script/Language Systems: 30
///   Unreferenced Features: ["test", "experimental"]
/// ```
fn print_feature_statistics(all_features: &BTreeSet<String>, referenced_features: &BTreeSet<String>) {
  let total_count = all_features.len();
  let referenced_count = referenced_features.len();
  // 全 Feature セットから参照済みセットを差し引いて未参照 Feature を検出
  let unreferenced_features: Vec<_> = all_features.difference(referenced_features).cloned().collect();

  println!();
  println!("Feature Statistics:");
  println!("  Total Features in GSUB/GPOS: {total_count}");
  println!("  Referenced in Script/Language Systems: {referenced_count}");
  println!("  Unreferenced Features: {unreferenced_features:?}");
}

/// Language System が参照する Feature タグ一覧を取得し、`referenced_features` に記録する
///
/// Feature タグを Language System 内のインデックス順で返します。
/// Character Variant（`cv01` 等）で名前付きパラメータが複数ある場合は、
/// タグに `(params: N)` を付加して返します（例：`"cv01 (params: 3)"`）。
///
/// # Arguments
///
/// * `lang_sys` - 対象の Language System
/// * `features` - Feature タグ取得に使用する `FeatureList`
/// * `referenced_features` - 参照済み Feature タグを記録するセット（統計処理で使用）
///
/// # Returns
///
/// Feature タグの文字列ベクタ（Language System 内のインデックス順）。
///
/// # Errors
///
/// - **`FeatureError`** - Feature Record の取得に失敗（インデックス範囲外など）
/// - **`FeatureParamsError`** - Feature パラメータ構造へのアクセス失敗
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

    if let Some(params) = feature.feature_params() {
      let feature_params = params.map_err(|source| ScriptLangsError::FeatureParamsError {
        feature_tag: feature_tag.clone(),
        source,
      })?;
      match feature_params {
        // StylisticSet・Size はパラメータ情報を表示しない
        FeatureParams::StylisticSet(_) | FeatureParams::Size(_) => {},

        // Character Variant は名前付きパラメータが複数あればその数をタグに付加
        // 例："cv01" → "cv01 (params: 3)"
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
