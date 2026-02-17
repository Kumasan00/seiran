#![allow(unused_assignments)]

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

use miette::{Diagnostic, IntoDiagnostic};
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
  #[error("Failed to read font file: {0}")]
  #[diagnostic(code(script_langs::io_error), help("Verify that the file exists and you have read permissions"))]
  IoError(#[from] std::io::Error),

  /// フォント解析エラー
  ///
  /// フォントファイルは読み込めましたが、指定インデックスのフォントを解析できません。
  /// TTC ファイルの場合、範囲外のインデックスを指定した可能性があります。
  #[error("Failed to parse font at index {font_index}")]
  #[diagnostic(
    code(script_langs::font_parse_error),
    help("Ensure the file is a valid font file (TTF/OTF/TTC/OTC). For font collections, try a different index.")
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
  #[error("GSUB table not found or invalid")]
  #[diagnostic(
    code(script_langs::gsub_error),
    help(
      "This font may not contain a GSUB (Glyph Substitution) table, which means it has no glyph substitution features"
    )
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
  #[error("GPOS table not found or invalid")]
  #[diagnostic(
    code(script_langs::gpos_error),
    help("This font may not contain a GPOS (Glyph Positioning) table, which means it has no positioning features")
  )]
  GposError {
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },

  /// Feature リスト取得エラー
  ///
  /// GSUB/GPOS テーブル内の Feature リスト構造が破損しているか、アクセスできません。
  #[error("Failed to retrieve feature list from {table_name} table")]
  #[diagnostic(
    code(script_langs::feature_list_error),
    help("The {table_name} table structure may be corrupted. Try validating the font file")
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
  #[error("Failed to retrieve script list from {table_name} table")]
  #[diagnostic(
    code(script_langs::script_list_error),
    help("The {table_name} table structure may be corrupted. Try validating the font file")
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
  #[error("Failed to retrieve language system at index {index} for script '{script_tag}'")]
  #[diagnostic(
    code(script_langs::lang_sys_error),
    help("The language system entry may be invalid or the script list structure is corrupted")
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
  #[error("Failed to retrieve feature at index {index}")]
  #[diagnostic(
    code(script_langs::feature_error),
    help("The feature table entry may be invalid. The feature list index might be out of bounds")
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
  #[error("Failed to retrieve feature parameters for feature '{feature_tag}'")]
  #[diagnostic(code(script_langs::feature_params_error), help("The feature parameters structure may be corrupted"))]
  FeatureParamsError {
    /// Feature タグ
    feature_tag: String,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
}

/// OpenType Layout テーブルの Script/Language システムを表示
///
/// 指定されたフォントの GSUB（Glyph Substitution）と GPOS（Glyph Positioning）テーブルから、
/// 対応する Script（文字体系）と Language System（言語）の組み合わせを取得して標準出力に表示します。
/// さらに、各 Script/Language に関連付けられた OpenType Feature の一覧を表示します。
///
/// 最後に、フォント全体で定義されている Feature 統計情報を表示します：
/// - 全 Feature 数
/// - Script/Language System から参照されている Feature 数
/// - 参照されていない「孤立した」Feature の一覧
///
/// # Arguments
///
/// * `file_path` - フォントファイルへのパス
///   (例：`NotoSerifJP.otf`、`SourceHanCodeJP.ttc`)
/// * `font_index` - TTC またはコレクション内のフォント インデックス
///   (通常は 0、TTC の場合は必要に応じて 1 以上を指定)
///
/// # Returns
///
/// - `Ok(())` - Script/Language 情報の表示に成功
/// - `Err(miette::Diagnostic)` - ファイル読み込み、解析、またはテーブルアクセス失敗
///
/// # Errors
///
/// 以下のいずれかで失敗します：
///
/// - **ファイル I/O エラー** - フォントが見つからない、読み込み権限がない
/// - **フォント解析エラー** - 指定インデックスのフォントが見つからない、形式が不正
/// - **GSUB/GPOS テーブル欠落** - テーブルが見つからないまたは無効
/// - **スクリプトリスト取得失敗** - Script リスト構造が破損
/// - **言語システム取得失敗** - Language System へのアクセス失敗
/// - **Feature 取得失敗** - Feature テーブル アクセス失敗
///
/// # Output Format
///
/// 標準出力に以下の形式でテキストを出力します：
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
/// ## 出力例
///
/// ```text
/// GSUB Table:
///   Script: latn
///     Default Language System: ["liga", "dlig", "calt"]
///     JAN: ["liga", "dlig", "calt", "fwid"]
///   Script: arab
///     Default Language System: ["init", "medi", "fina", "isol"]
///
/// GPOS Table:
///   Script: latn
///     Default Language System: ["kern", "mark"]
///
/// Feature Statistics:
///   Total Features in GSUB/GPOS: 12
///   Referenced in Script/Language Systems: 11
///   Unreferenced Features: ["test"]
/// ```
///
/// # Example
///
/// ```ignore
/// seiran script-langs fonts/NotoSerifJP.otf
/// ```
pub fn script_langs(file_path: &Path, font_index: u32) -> miette::Result<()> {
  // [0] デバッグ用ロギング - 処理対象ファイルとインデックスを記録
  // 複数フォント含むファイルの場合、インデックスにより処理対象を一意に特定
  info!(font_file_path = %file_path.display(), font_index = font_index, "Input font file path and index");

  // [0.1] Feature 参照追跡用セットの初期化
  // 後の処理で Script/Language System から参照される Feature タグを累積記録
  // BTreeSet により重複が自動排除される
  let mut referenced_features = BTreeSet::new();

  // [0.2] フォントファイルをメモリ全体に読み込み
  // メモリマップではなく完全読み込みとすることで、後続テーブル解析を効率化
  let font_data = fs::read(file_path).into_diagnostic()?;

  // [0.3] 指定インデックスのフォント参照を取得
  // TTC/OTC の場合、font_index により複合ファイル内から特定フォントを抽出
  // シングル TTF/OTF の場合は通常 font_index=0
  let font_ref = FontRef::from_index(&font_data, font_index)
    .map_err(|source| ScriptLangsError::FontParseError { font_index, source })?;

  // [1] GSUB (Glyph Substitution) テーブル処理
  // グリフ置換機能（"liga"=合字、"dlig"=選択合字など）の Script/Language 対応状況を解析
  let gsub = font_ref.gsub().map_err(|source| ScriptLangsError::GsubError { source })?;
  let gsub_features = process_layout_table("GSUB", gsub.feature_list(), gsub.script_list(), &mut referenced_features)?;

  // [2] GPOS (Glyph Positioning) テーブル処理
  // グリフ位置調整機能（"kern"=カーニング、"mark"=アクセント位置調整など）の Script/Language 対応状況を解析
  let gpos = font_ref.gpos().map_err(|source| ScriptLangsError::GposError { source })?;
  let gpos_features = process_layout_table("GPOS", gpos.feature_list(), gpos.script_list(), &mut referenced_features)?;

  // [3] Feature 統計情報の収集と表示
  // GSUB と GPOS から抽出した全 Feature を統合し、以下を検出・表示：
  // - 全 Feature 数（GSUB+GPOS の合計、重複排除）
  // - 参照 Feature 数（実際に使用されている Feature）
  // - 未参照 Feature（定義されているが使用されていない Feature - バグの可能性）
  let all_features = collect_all_features(&gsub_features, &gpos_features);
  print_feature_statistics(&all_features, &referenced_features);

  return Ok(());
}

/// OpenType Layout テーブル（GSUB/GPOS）の Script/Language System を処理
///
/// OpenType Layout テーブルの `ScriptList` を走査して、サポートされているすべての
/// `Script`（文字体系）と `Language System`（言語システム）の組み合わせを出力します。
/// 各組み合わせに関連付けられた `OpenType Feature` を特定して記録します。
///
/// この関数は以下の処理を実行します：
///
/// 1. テーブルヘッダーを表示（"GSUB Table:" または "GPOS Table:"）
/// 2. `Script` ごとに `Default Language System` と言語固有 `Language System` を反復処理
/// 3. 各 `Language System` に関連付けられた `Feature` タグを取得
/// 4. 参照された `Feature` を `BTreeSet` に追加（後の統計処理で使用）
/// 5. 処理済みの `FeatureList` を呼び出し元に返却
///
/// # Arguments
///
/// * `table_name` - OpenType テーブル名（"GSUB" または "GPOS"）
///   表示用テーブルヘッダーとして使用
/// * `feature_list` - `Result<FeatureList>` - Feature テーブルの参照
///   Feature タグの取得に使用
/// * `script_list` - `Result<ScriptList>` - Script テーブルの参照
///   Script/Language System の反復処理に使用
/// * `referenced_features` - 参照された `Feature` タグの `BTreeSet`
///   関数内で拡張され、統計処理で使用される `Feature` が記録されます
///
/// # Returns
///
/// 成功時に `FeatureList<'a>` を返します。呼び出し元で `Feature` 統計処理に使用できます。
/// 型ライフタイムは元のフォント データへの参照に紐付けられます。
///
/// # Errors
///
/// 以下の場合にエラーを返します：
///
/// - **`FeatureListError`** - Feature リスト構造が破損または読み取り不可
/// - **`ScriptListError`** - Script リスト構造が破損または読み取り不可
/// - **`LangSysError`** - Language System へのアクセスに失敗
/// - **`FeatureError`** - Feature タグの取得に失敗
///
/// # Example Output (GSUB Table)
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
  // テーブルヘッダーを表示
  println!("{table_name} Table:");

  // [1] Feature リスト構造の取得 - 後で各 `Feature` タグを取得する際に必要
  let feature_list = feature_list.map_err(|source| ScriptLangsError::FeatureListError { table_name, source })?;

  // [2] Script リスト構造の取得 - Script/Language System を列挙するための基盤
  let script_list = script_list.map_err(|source| ScriptLangsError::ScriptListError { table_name, source })?;

  // [3] Script/Language と `Feature` の関連付けを表示
  // print_scripts 内で `Script` を走査し、各 Language System が参照する `Feature` を取得
  print_scripts(&script_list, &feature_list, referenced_features)?;

  return Ok(feature_list);
}

/// Script ごとの `Language System` と `Feature` を表示
///
/// OpenType Layout テーブルの `ScriptList` を走査し、各 `Script` に対して以下の情報を出力します：
///
/// 1. `Script` タグ（"latn"、"arab"、"cyrl" など）
/// 2. `Default Language System` に関連付けられた `Feature`（すべての言語で共通）
/// 3. 言語固有 `Language System` と該当する `Feature`
///    - 例："JAN"（日本語）、"KOR"（韓国語）
///
/// 各 `Language System` の `Feature` は `get_language_features()` を通じて取得され、
/// 参照された `Feature` は `referenced_features` に記録されます。
///
/// # Arguments
///
/// * `scripts` - `ScriptList` 参照 - 走査対象の `Script` レコード群
/// * `features` - `FeatureList` 参照 - `Feature` タグ取得用
/// * `referenced_features` - 参照された `Feature` を記録する可変 `BTreeSet`
///   この関数内で拡張されます
///
/// # Returns
///
/// 成功時に `Ok(())` を返します。
/// 表示は標準出力に直接出力されます。
///
/// # Errors
///
/// 以下の場合にエラーを返します：
///
/// - **`LangSysError`** - `Language System` レコードへのアクセス失敗
///   例：テーブル構造が破損、オフセット無効
/// - **`FeatureError`** - `Feature` タグ取得に失敗
///   例：`Feature` インデックスが範囲外
///
/// # Output Format
///
/// 標準出力に以下の形式で出力：
///
/// ```text
///   Script: latn
///     Default Language System: ["liga", "dlig", "calt"]
///     JAN: ["liga", "dlig", "calt", "fwid"]
/// ```
///
/// ### 出力例（複数言語対応フォント）
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
  // [1] すべての Script レコードを走査
  for script_record in scripts.script_records() {
    // [1.1] Script タグを抽出（例："latn"、"arab"）
    let script_tag = script_record.script_tag().to_string();
    println!("  Script: {script_tag}");

    // [1.2] Script テーブルを取得（Script 内の言語システム定義を含む）
    if let Ok(subtable) = script_record.script(scripts.offset_data()) {
      // [1.3] Default Language System の処理
      // すべての言語で共通して使用される Feature を定義（存在する場合）
      if let Some(default_lang_sys) = subtable.default_lang_sys() {
        let default_lang = default_lang_sys.map_err(|source| ScriptLangsError::LangSysError {
          index: 0,
          script_tag: script_tag.clone(),
          source,
        })?;
        // Default Language System に属する Feature を取得し、参照 Feature に追加
        let feature_tags = get_language_features(&default_lang, features, referenced_features)?;
        println!("    Default Language System: {feature_tags:?}");
      }

      // [1.4] 言語固有 Language System の処理
      // 各言語タグ（"JAN"、"KOR"など）に対応する Language System を処理
      for lang_record in subtable.lang_sys_records() {
        if let Ok(lang_sys) = lang_record.lang_sys(subtable.offset_data()) {
          // 言語タグを取得（例："JAN" = 日本語、"KOR" = 韓国語）
          let lang_tag = lang_record.lang_sys_tag().to_string();
          // 言語固有 Language System に属する Feature を取得し、参照 Feature に追加
          let feature_tags = get_language_features(&lang_sys, features, referenced_features)?;
          println!("    {lang_tag}: {feature_tags:?}");
        }
      }
    }
  }
  return Ok(());
}

/// `GSUB` と `GPOS` テーブルから全 `Feature` タグを収集
///
/// 指定された `GSUB`（Glyph Substitution）と `GPOS`（Glyph Positioning）テーブルから
/// すべての `Feature` タグを抽出し、重複を排除した一意なタグのセットを返します。
///
/// OpenType フォントでは、同じ `Feature` タグが `GSUB` と `GPOS` の両方に定義される場合があります
/// （例：「kern」は GPOS でカーニング、GSUB で代替グリフの選択に使用）。
/// この関数は両方のテーブルを統合して、フォント全体の Feature インベントリを作成します。
///
/// 処理フロー：
/// 1. `GSUB` テーブルから全 `Feature` タグを抽出
/// 2. `GPOS` テーブルから全 `Feature` タグを抽出
/// 3. `BTreeSet` に統合（自動的に重複排除）
/// 4. アルファベット順にソートされたセットを返却
///
/// # Arguments
///
/// * `gsub_features` - `GSUB` の `FeatureList` 参照
///   Glyph Substitution テーブルのすべての `Feature` を含む
/// * `gpos_features` - `GPOS` の `FeatureList` 参照
///   Glyph Positioning テーブルのすべての `Feature` を含む
///
/// # Returns
///
/// 返された `BTreeSet<String>` には：
/// - GSUB と GPOS から抽出されたすべての一意な Feature タグ
/// - タグはアルファベット順に自動ソート（BTreeSet 特性）
/// - 重複は自動的に削除
///
/// # Example
///
/// GSUB で [`"liga"`, `"dlig"`] を含む、GPOS で [`"kern"`, `"liga"`] を含む場合：
/// 戻り値は `BTreeSet { "dlig", "kern", "liga" }`
///
/// # ユースケース
///
/// - Font Feature 統計情報の作成
/// - Script/Language System から参照されていない「孤立」`Feature` の検出
/// - フォント `Feature` インベントリの完全な把握
fn collect_all_features(
  gsub_features: &FeatureList,
  gpos_features: &FeatureList,
) -> std::collections::BTreeSet<std::string::String> {
  let mut all_features = BTreeSet::new();

  // [1] GSUB テーブルからすべての Feature タグを抽出
  insert_feature_tags(gsub_features, &mut all_features);

  // [2] GPOS テーブルからすべての Feature タグを抽出
  // BTreeSet の特性により、GSUB と重複するタグは自動的に1回のみ登録される
  insert_feature_tags(gpos_features, &mut all_features);

  return all_features;
}

/// `FeatureList` から `Feature` タグを抽出してセットに追加
///
/// 指定された `FeatureList` のすべての Feature Record を走査し、
/// Feature タグを文字列に変換してセットに追加します。
/// すでに同じタグがセットに存在する場合、BTreeSet の特性により
/// 自動的に重複が排除されます。
///
/// 処理フロー：
/// 1. `FeatureList` イテレータを取得
/// 2. 各 `Feature Record` を走査
/// 3. `Feature` タグ（4文字識別子）を文字列に変換
/// 4. `BTreeSet` に挿入（重複排除）
///
/// # Arguments
///
/// * `feature_list` - 抽出対象の `FeatureList` 参照
///   (GSUB または GPOS テーブルよる)
/// * `all_features` - 収集先の可変 `BTreeSet<String>` 参照
///   この集合に新しいタグが追加されます
///
/// # Example
///
/// `GSUB FeatureList` に `["liga", "dlig", "calt"]` 含む場合、
/// `all_features` : {"liga", "dlig", "calt"} に展開されます。
///
/// # 呼び出し元
///
/// `collect_all_features()` 内部ヘルパー - 通常は直接呼び出す必要なし
fn insert_feature_tags(feature_list: &FeatureList, all_features: &mut BTreeSet<String>) {
  // Feature Record を反復処理
  for record in feature_list.feature_records() {
    // Feature タグ（4文字識別子）を文字列に変換
    let tag = record.feature_tag().to_string();
    // BTreeSet に挿入 - 同じタグが存在する場合は何もしない（重複排除）
    all_features.insert(tag);
  }
}

/// OpenType Feature 統計情報を標準出力に表示
///
/// `GSUB` と `GPOS` テーブル全体の `Feature` 統計情報を表示します。
/// これらの統計情報は、フォントが実装している `Feature` と実際に使用されている
/// `Feature` のギャップを特定するのに役立ちます（例：未使用 `Feature` の検出）。
///
/// 表示される統計情報：
///
/// 1. **Total Features in GSUB/GPOS**（全 `Feature` 数）
///    - GSUB と GPOS に定義されているすべての一意な `Feature` の総数
///    - 重複は1回のみカウント
///
/// 2. **Referenced in Script/Language Systems**（参照 `Feature` 数）
///    - 少なくとも 1 つの Script/Language System から参照されている `Feature`
///    - 実際に使用されている可能性が高い `Feature`
///
/// 3. **Unreferenced Features**（未参照 Feature 一覧）
///    - Script/Language System から参照されていない `Feature`
///    - バグの可能性、または言語サポートが欠落している可能性
///    - 例：`["test", "experimental"]`
///
/// 処理フロー：
/// 1. 全 `Feature` セットから統計情報を計算
/// 2. 未参照 `Feature` を差分計算（全 Feature - 参照 Feature）
/// 3. 標準出力に見やすく表示
///
/// # Arguments
///
/// * `all_features` - GSUB/GPOS の全 `Feature` タグセット
/// * `referenced_features` - Script/Language System から参照される `Feature` タグセット
///
/// # Output Format
///
/// 標準出力に以下の形式で出力：
///
/// ```text
/// 
/// Feature Statistics:
///   Total Features in GSUB/GPOS: 32
///   Referenced in Script/Language Systems: 30
///   Unreferenced Features: ["test", "experimental"]
/// ```
///
/// 未参照 Feature がない場合のOutput：
///
/// ```text
/// 
/// Feature Statistics:
///   Total Features in GSUB/GPOS: 32
///   Referenced in Script/Language Systems: 32
///   Unreferenced Features: []
/// ```
///
/// # Example Interpretation
///
/// - Total 32、Referenced 30 → 2 つの未使用 `Feature` がある（`["test", "experimental"]`）
/// - Total 32、Referenced 32 → すべての `Feature` が使用されている（品質が高い）
/// - Total 5、Referenced 3 → 設計不完全の可能性
///
/// # ユースケース
///
/// - フォント品質の確認（未使用 `Feature` の検出）
/// - フォント最適化（不要 `Feature` の特定）
/// - バグ検出（予期しない未参照 `Feature`）
fn print_feature_statistics(all_features: &BTreeSet<String>, referenced_features: &BTreeSet<String>) {
  // [1] 統計値を計算
  let total_count = all_features.len(); // GSUB/GPOS で定義されているすべての `Feature` 数
  let referenced_count = referenced_features.len(); // Script/Language System で実際に参照される `Feature` 数

  // [2] 未参照 `Feature` を検出
  // 全 `Feature` セットから参照 `Feature` セットを差分計算（全 Feature - 参照 Feature = 未参照 Feature）
  let unreferenced_features: Vec<_> = all_features.difference(referenced_features).cloned().collect();

  // [3] 統計情報を標準出力に表示
  // 見やすくするため空行を挿入
  println!();
  println!("Feature Statistics:");
  println!("  Total Features in GSUB/GPOS: {total_count}");
  println!("  Referenced in Script/Language Systems: {referenced_count}");
  println!("  Unreferenced Features: {unreferenced_features:?}");
}

/// 言語システムに関連付けられた Feature タグ一覧を取得
///
/// 指定された Language System（LangSys）に関連付けられたすべての `Feature` を取得し、
/// そのタグ一覧を返します。同時に、参照追跡用セットに `Feature` を記録します。
///
/// `FeatureList` からインデックスベースで `Feature` を取得し、タグを抽出します。
/// `Feature` にパラメータが存在する場合（複数のバリアント機能）は、
/// タグに追加情報を付加して返却します。
///
/// 処理フロー：
///
/// 1. `Language System` の `Feature` インデックスリストを反復処理
/// 2. 各 `Feature` インデックスから `Feature Record` を取得
/// 3. `Feature` タグを抽出して列挙
/// 4. `Feature` パラメータが存在するか確認：
///    - **Stylistic Set**: スタイリッシック代替グリフセット（追加情報なし）
///    - **Size**: フォントサイズ最適化用（追加情報なし）
///    - **Character Variant**: 複数の名前付きパラメータ対応
///      → パラメータ数が複数の場合、タグに追加情報を付加
///      → 例："cv01" → "cv01 (params: 3)"
/// 5. 参照 `Feature` セットに追加（統計処理で使用）
/// 6. タグリストを返却
///
/// # Arguments
///
/// * `lang_sys` - `Language System` 参照
///   ("Default Language System" または "JAN"、"KOR" などの言語タグ)
/// * `features` - `FeatureList` 参照
///   `Feature` タグと情報抽出用
/// * `referenced_features` - 参照 `Feature` を記録する可変セット
///   統計処理で未参照 `Feature` を検出するために使用
///
/// # Returns
///
/// 成功時に Feature タグの文字列ベクタを返します：
/// - 形式：`"liga"`、`"dlig"`、`"kern"`、`"cv01 (params: 3)"` など
/// - 順序：Language System 内のインデックス順序を保持
/// - パラメータ情報：Character Variant の複数パラメータ が記載される
///
/// # Errors
///
/// 以下の場合にエラーを返します：
///
/// - **`FeatureError`** - `Feature Record` 取得に失敗
///   例：インデックスが範囲外、テーブル構造破損
/// - **`FeatureParamsError`** - `Feature` パラメータアクセス失敗
///   例：パラメータ構造が不正、読み取り不可
///
/// # Example
///
/// 入力：Default Language System が Feature インデックス [0, 1, 3] を参照
/// 出力：`["liga", "dlig", "calt"]`
///
/// 出力（Character Variant パラメータ付き）
/// 入力：Language System が Feature インデックス [5] を参照（cv01、3 パラメータ）
/// 出力：`["cv01 (params: 3)"]`
///
/// # ユースケース
///
/// - Script/Language ごとの Feature インベントリ表示
/// - Feature 統計情報の作成（参照追跡）
/// - 複雑な Feature パラメータを持つフォント検査
fn get_language_features(
  lang_sys: &LangSys,
  features: &FeatureList,
  referenced_features: &mut BTreeSet<String>,
) -> Result<Vec<String>, ScriptLangsError> {
  let mut feature_tags = Vec::new();

  // [1] Language System に属するすべての Feature インデックスを走査
  for feature_index in lang_sys.feature_indices() {
    // [1.1] Feature インデックスから Feature Record を取得
    let feature = features.get(feature_index.get()).map_err(|source| ScriptLangsError::FeatureError {
      index: feature_index.get(),
      source,
    })?;

    // [1.2] Feature タグを抽出し、参照 Feature セットに記録（統計処理で使用）
    let mut feature_tag = feature.tag.to_string();
    referenced_features.insert(feature_tag.clone());

    // [1.3] Feature パラメータの処理
    // 一部の Feature は追加パラメータを持ち、複数のバリアント機能をサポート
    if let Some(params) = feature.feature_params() {
      let feature_params = params.map_err(|source| ScriptLangsError::FeatureParamsError {
        feature_tag: feature_tag.clone(),
        source,
      })?;
      match feature_params {
        // Stylistic Set: 代替グリフスタイル（パラメータ情報は表示しない）
        // Size: フォントサイズ最適化（パラメータ情報は表示しない）
        FeatureParams::StylisticSet(_) | FeatureParams::Size(_) => {},

        // Character Variant: 複数の名前付きパラメータをサポート
        // パラメータ数が複数の場合、ユーザーに情報を提供するため Tag に付加
        // 例："cv01" → "cv01 (params: 3)" (3つのバリアントが利用可能)
        FeatureParams::CharacterVariant(character_variant) => {
          let param_count = character_variant.num_named_parameters();
          if param_count > 1 {
            // 複数のパラメータが存在する場合、その数を Tag に追加
            feature_tag = format!("{feature_tag} (params: {param_count})");
          }
        },
      }
    }
    // [1.4] 処理済み Feature タグを結果リストに追加
    feature_tags.push(feature_tag);
  }
  return Ok(feature_tags);
}
