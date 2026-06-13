//! TOML 設定ファイルのパース・検証・変換モジュール
//!
//! PDF 生成に必要な設定情報を TOML から読み込み、`garde` による宣言的バリデーション、
//! ファイルパスの解決、型変換を実行して構造化設定データを構築します。

use std::{
  fs,
  path::{Path, PathBuf},
};

use garde::Validate;
use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;
use tracing::{info, warn};
use types::FontType;

mod pre_config;
use pre_config::{PreConfig, PreFontConfig};
mod processed_config;
mod tag;

#[doc(hidden)]
pub mod test_support;

pub use processed_config::{
  Config, DocumentConfig, Feature, FontConfig, FontConfigs, Margin, OutputConfig, PdfConfig, TextDirection,
  VariationAxis,
};

/// 設定ファイル読み込みで発生するすべてのエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum ReadConfigError {
  /// 設定ファイルの読み込み失敗
  #[error("設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// TOML 解析失敗
  ///
  /// `toml::de::Error` 自身も `Display` で line/column の自前スニペットを描画するため、
  /// miette の `#[label]` と二重に出ないよう `parse_config` 内で `set_input(None)` により
  /// 内蔵 input をクリアして抑止する。位置表示は miette の `#[source_code]` + `#[label]` に
  /// 一本化する。
  #[error("設定ファイルの TOML 解析に失敗しました")]
  #[diagnostic(code(config::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    #[source_code]
    src: NamedSource<String>,
    #[label("ここ")]
    span: SourceSpan,
    #[source]
    source: toml::de::Error,
  },
  /// カレントディレクトリの取得失敗
  #[error("カレントディレクトリを取得できませんでした。")]
  #[diagnostic(code(config::current_dir), help("プロセスの作業ディレクトリが有効か確認してください。"))]
  CurrentDir {
    #[source]
    source: std::io::Error,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("複数のバリデーションエラーが発生しました。")]
  #[diagnostic(code(config::multiple_validation_errors))]
  MultipleValidationErrors {
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// 設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// garde が検出した設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(config::validation::field), help("config.toml の該当フィールドの値を確認してください。"))]
  Field { path: String, message: String },
  /// フォントパスを解決できない
  #[error("フォントファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::font_path),
    help("フォントファイルが存在し、読み取り権限があることを確認してください。")
  )]
  FontPathResolution {
    font_type: FontType,
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// スタイル設定ファイルのパスを解決できない
  #[error("スタイル設定ファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::style_path),
    help("スタイル設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  StylePathResolution {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// 参照設定ファイルのパスを解決できない
  #[error("参照設定ファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::references_path),
    help("参照設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  ReferencesPathResolution {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// ソースファイルのパスを解決できない
  #[error("ソースファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::source_path),
    help("`sources` に列挙したファイルが存在し、読み取り権限があることを確認してください。")
  )]
  SourcePathResolution {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// 出力ディレクトリの作成失敗
  #[error("出力ディレクトリを作成できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::create_output_dir),
    help("親ディレクトリが存在し、書き込み権限があることを確認してください。")
  )]
  CreateOutputDir {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// 出力ディレクトリのパス正規化失敗
  #[error("出力ディレクトリのパスを正規化できませんでした: {path}")]
  #[diagnostic(
    code(config::validation::canonicalize_output_dir),
    help("指定したディレクトリが存在するか確認してください。")
  )]
  CanonicalizeOutputDir {
    path: String,
    #[source]
    source: std::io::Error,
  },
}

/// 読み取り I/O フェーズで集約する解決済みパス群。
///
/// [`resolve_paths`] がフォント・ソース・スタイル・参照の各パスを `canonicalize` し、
/// 成功したものはここに詰め、失敗したものは別途返す [`Vec<ValidationError>`] に集約します。
/// `font_paths` は [`FontType::ALL`] の順序に対応する正規化済みフォントパスで、エラーが
/// 1 件もない場合のみ 19 要素が揃います（[`resolve`] はエラー時に早期 return するため、
/// 組み立て時には必ず揃っています）。タグ・書字方向の変換は I/O に依存しない純粋処理として
/// [`validate_and_convert`] が別途担当し、このフェーズはパス解決のみを行います。
struct ResolvedPaths {
  font_paths: Vec<PathBuf>,
  sources: Vec<PathBuf>,
  style_path: Option<PathBuf>,
  references_path: Option<PathBuf>,
}

/// フォント 1 種別ぶんの、パス解決を除いた検証済み・変換済みの値群。
///
/// [`parse_font_values`] が純粋に（I/O なしで）生成し、[`resolve`] が正規化済みの
/// `font_path` と zip して [`FontConfig`] に組み立てます。フィールドは `font_path` を
/// 除いて [`FontConfig`] と一対一に対応します。
struct FontValues {
  /// `PDF FontDescriptor` で使用されるフォント名
  font_name: String,
  /// TTC ファイル内のフォントインデックス
  font_index: u32,
  /// バリアブルフォント軸の設定値（タグ変換済み）
  variation_axes: Option<Vec<VariationAxis>>,
  /// OpenType / ISO 15924 script タグ（4 バイト、case 保持）
  script: Option<[u8; 4]>,
  /// harfrust に渡す最終 BCP 47 言語文字列
  language: Option<String>,
  /// OpenType 言語システムタグ（4 バイトに正規化済み）
  ot_language_tag: Option<[u8; 4]>,
  /// 書字方向
  direction: Option<TextDirection>,
  /// OpenType フィーチャー設定（タグ変換済み）
  features: Option<Vec<Feature>>,
}

/// 指定パスから設定ファイルを読み込みます。
///
/// [`parse_config`] → [`resolve`] を連結する利便ラッパで、生産コードから使う想定です。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション・出力パス構築の失敗時にエラーを返します。
// `ReadConfigError::ParseToml` が `NamedSource<String>` を保持して Result サイズが拡大するため
// allow する。`config.toml` は 1 回しか読まないので最適化対象ではない。
#[allow(clippy::result_large_err)]
pub fn read_config(config_path: &Path) -> Result<Config, ReadConfigError> {
  info!(config_path = %config_path.display(), "設定ファイルの読み込みを開始します");
  let config_content = fs::read_to_string(config_path).map_err(|source| ReadConfigError::ReadFile {
    path: config_path.display().to_string(),
    source,
  })?;
  let pre_config = parse_config(&config_content, config_path)?;
  let current_dir = std::env::current_dir().map_err(|source| ReadConfigError::CurrentDir { source })?;
  let config = resolve(pre_config, &current_dir)?;

  info!(
    config_path = %config_path.display(),
    output_name = %config.output.name,
    output_path = %config.output.pdf_path().display(),
    "設定ファイルの読み込みが完了しました"
  );
  return Ok(config);
}

/// TOML 文字列を [`PreConfig`] にパースします（I/O なし）。
///
/// `source_path` はエラー報告に使う表示用パスで、ファイルシステムへのアクセスには使われません。
/// 値検証は行いません。検証・変換は [`validate_and_convert`]（[`resolve`] 経由）で実行します。
///
/// # Errors
///
/// TOML の構文が不正な場合に [`ReadConfigError::ParseToml`] を返します。
#[allow(clippy::result_large_err)]
fn parse_config(content: &str, source_path: &Path) -> Result<PreConfig, ReadConfigError> {
  return toml::from_str(content).map_err(|mut source| {
    let span = source.span().map_or_else(
      || SourceSpan::new(0.into(), 0),
      |range| SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    // toml::de::Error::Display は input が設定されていると line/column の自前スニペットを描画する。
    // miette の #[label] と二重に位置情報が出るため、ここで input をクリアして抑止する。
    source.set_input(None);
    return ReadConfigError::ParseToml {
      src: NamedSource::new(source_path.display().to_string(), content.to_string()),
      span,
      source,
    };
  });
}

/// [`PreConfig`] からパス解決・出力ディレクトリ作成を行い [`Config`] を構築します。
///
/// 純粋値検証＋変換 → 読み取り I/O（パス解決）→ 書き込み I/O（出力ディレクトリ作成）の順で
/// 3 フェーズに分かれます。タグ・書字方向の検証は変換と一体化しており、純粋フェーズで 1 回だけ
/// パースされます（パス解決フェーズではタグに触れません）。純粋フェーズと読み取りフェーズの
/// 違反は [`ReadConfigError::MultipleValidationErrors`] に集約し、いずれかで失敗があれば
/// 書き込みフェーズをスキップして副作用を最小化します。
///
/// # Errors
///
/// 値検証・タグ変換・パス解決・出力ディレクトリ作成のいずれかに違反があった場合は
/// [`ReadConfigError::MultipleValidationErrors`] を返します。
#[allow(clippy::result_large_err)]
fn resolve(pre: PreConfig, current_dir: &Path) -> Result<Config, ReadConfigError> {
  let validation = validate_and_convert(&pre);
  let (resolved, path_errors) = resolve_paths(&pre, current_dir);

  // 純粋フェーズが成功し、かつパス解決にも違反がない場合のみ組み立てへ進む。
  // それ以外は両フェーズの違反をまとめて報告する。
  let font_values = match validation {
    Ok(font_values) if path_errors.is_empty() => font_values,
    result => {
      let mut errors = match result {
        Ok(_) => Vec::new(),
        Err(value_errors) => value_errors,
      };
      errors.extend(path_errors);
      return Err(ReadConfigError::MultipleValidationErrors { errors });
    },
  };

  let output_dir = match build_output_dir(current_dir, pre.output.output_dir.as_deref()) {
    Ok(dir) => dir,
    Err(error) => {
      return Err(ReadConfigError::MultipleValidationErrors {
        errors: vec![error],
      });
    },
  };

  let PreConfig {
    document: pre_document,
    output: pre_output,
    pdf: pre_pdf_config,
    ..
  } = pre;

  // FontType::ALL の順序で揃った検証済み値と正規化済みパスを zip して FontConfig を組み立てる。
  let font_configs =
    FontConfigs::from_all(font_values.into_iter().zip(resolved.font_paths).map(|(values, font_path)| FontConfig {
      font_name: values.font_name,
      font_path,
      font_index: values.font_index,
      variation_axes: values.variation_axes,
      script: values.script,
      language: values.language,
      ot_language_tag: values.ot_language_tag,
      direction: values.direction,
      features: values.features,
    }));

  return Ok(Config {
    document: DocumentConfig {
      title: pre_document.title,
      author: pre_document.author,
      date: pre_document.date,
      subject: pre_document.subject,
      language: pre_document.language,
      keywords: pre_document.keywords,
    },
    output: OutputConfig {
      name: pre_output.name,
      output_dir,
    },
    pdf: PdfConfig {
      height: pre_pdf_config.height,
      width: pre_pdf_config.width,
      margin: Margin {
        top: pre_pdf_config.margin_top,
        bottom: pre_pdf_config.margin_bottom,
        left: pre_pdf_config.margin_left,
        right: pre_pdf_config.margin_right,
      },
    },
    font_configs,
    sources: resolved.sources,
    style_path: resolved.style_path,
    references_path: resolved.references_path,
  });
}

/// [`PreConfig`] の純粋な値検証とタグ・書字方向の変換を一括で実行します（I/O なし）。
///
/// `garde` のフィールド検証、余白合計・`font_name` 重複・`ot_language`⇒`script` の相互制約、
/// および 19 フォント種別ぶんのタグ・書字方向の構造的検証＋変換（[`parse_font_values`]）を
/// すべて 1 度に集約します。タグ・書字方向の検証はここで唯一行われるため、変換と検証が
/// 二重実行されることはありません。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
/// 違反が皆無の場合は [`FontType::ALL`] の順序に対応する [`FontValues`] のベクタを返します。
fn validate_and_convert(pre: &PreConfig) -> Result<Vec<FontValues>, Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();
  if let Err(report) = pre.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }
  pre_config::validate_margin_sums(&pre.pdf, &mut errors);
  pre_config::validate_unique_font_names(&pre.font_configs, &mut errors);
  pre_config::validate_font_language_constraints(&pre.font_configs, &mut errors);

  let mut font_values: Vec<FontValues> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    match parse_font_values(font_type, pre.font_configs.get(font_type)) {
      Ok(values) => font_values.push(values),
      Err(value_errors) => errors.extend(value_errors),
    }
  }

  if errors.is_empty() {
    return Ok(font_values);
  }
  return Err(errors);
}

/// 純粋な値検証のみを行うテスト向けラッパ（変換結果は破棄）。
///
/// 既存のユニットテストが「値検証だけ」を観察できるよう [`validate_and_convert`] を薄く包みます。
/// 生産コードのパスでは [`resolve`] が [`validate_and_convert`] を直接使用します。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
#[cfg(test)]
fn validate_values(pre: &PreConfig) -> Result<(), Vec<ValidationError>> {
  return validate_and_convert(pre).map(|_| ());
}

/// 読み取り I/O フェーズ: フォント・ソース・スタイル・参照のパスを解決します。
///
/// 各パスを独立に `canonicalize` し、解決できたものは [`ResolvedPaths`] に詰め、失敗した
/// ものは [`ValidationError`] として集約して返します。書き込み I/O は行わないため、ここで
/// 失敗があっても呼び出し側が安全に中断できます。タグ・書字方向の変換は純粋処理として
/// [`parse_font_values`] が担当するため、このフェーズはパス解決のみに専念します。
///
/// `font_paths` は [`FontType::ALL`] の順序で `canonicalize` に成功したものだけを詰めます。
/// 1 件でも失敗があれば [`resolve`] は組み立て前に早期 return するため、組み立て時には
/// 必ず 19 要素が順序どおり揃っています。
fn resolve_paths(pre: &PreConfig, current_dir: &Path) -> (ResolvedPaths, Vec<ValidationError>) {
  let mut errors: Vec<ValidationError> = Vec::new();

  let style_path = canonicalize_or_record(pre.style_path.as_deref(), &mut errors, |path, source| {
    ValidationError::StylePathResolution { path, source }
  });
  let references_path = canonicalize_or_record(pre.references_path.as_deref(), &mut errors, |path, source| {
    ValidationError::ReferencesPathResolution { path, source }
  });

  let mut font_paths: Vec<PathBuf> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    let pre_font_config = pre.font_configs.get(font_type);
    match pre_font_config.font_path.canonicalize() {
      Ok(font_path) => font_paths.push(font_path),
      Err(source) => errors.push(ValidationError::FontPathResolution {
        font_type,
        path: pre_font_config.font_path.display().to_string(),
        source,
      }),
    }
  }

  let sources = canonicalize_sources(&pre.sources, current_dir, &mut errors);

  return (
    ResolvedPaths {
      font_paths,
      sources,
      style_path,
      references_path,
    },
    errors,
  );
}

/// オプションのパスを `canonicalize` し、失敗時はエラーを `errors` に追加します。
fn canonicalize_or_record(
  path: Option<&Path>,
  errors: &mut Vec<ValidationError>,
  make_err: impl FnOnce(String, std::io::Error) -> ValidationError,
) -> Option<PathBuf> {
  match path {
    Some(p) => match p.canonicalize() {
      Ok(canon) => return Some(canon),
      Err(source) => {
        errors.push(make_err(p.display().to_string(), source));
        return None;
      },
    },
    None => return None,
  }
}

/// 各 source パスを `canonicalize` し、失敗時はエラーを `errors` に追加する。
fn canonicalize_sources(sources: &[PathBuf], current_dir: &Path, errors: &mut Vec<ValidationError>) -> Vec<PathBuf> {
  let mut resolved = Vec::with_capacity(sources.len());
  for source_path in sources {
    if source_path.extension().and_then(|ext| ext.to_str()) != Some("sei") {
      warn!(
        path = %source_path.display(),
        "ソースファイルの拡張子が `.sei` ではありません。Seiran は `.sei` 拡張子を推奨します。"
      );
    }
    let absolute = if source_path.is_absolute() {
      source_path.clone()
    } else {
      current_dir.join(source_path)
    };
    match absolute.canonicalize() {
      Ok(canon) => resolved.push(canon),
      Err(source) => errors.push(ValidationError::SourcePathResolution {
        path: absolute.display().to_string(),
        source,
      }),
    }
  }
  return resolved;
}

/// `PreFontConfig` のタグ・書字方向を検証・変換し、[`FontValues`] を生成します（I/O なし）。
///
/// `script` / `ot_language` / `direction` / フィーチャー / バリアブル軸の各タグを
/// [`crate::tag`] の失敗しうるコンストラクタ・[`TextDirection`] の `FromStr` 実装でパースします。
/// これらの関数が検証と `[u8; 4]` / enum への変換を兼ねるため、ここがタグ・書字方向の
/// 唯一の検証点です（旧 `four_byte_tag` の `unwrap()` による長さ不正パニックや、検証と変換の
/// ドリフトは生じません）。`font_path` の解決は I/O フェーズ（[`resolve_paths`]）が担当します。
///
/// # Errors
///
/// 1 つ以上のタグ・書字方向が不正な場合、当該フォント種別の [`ValidationError::Field`] を
/// すべて集約して `Err` で返します（途中で打ち切らず、複数の不正を 1 度に報告します）。
fn parse_font_values(font_type: FontType, pre_font_config: &PreFontConfig) -> Result<FontValues, Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();

  let script = match pre_font_config.script.as_deref() {
    None => None,
    Some(value) => match tag::parse_script_tag(value) {
      Ok(bytes) => Some(bytes),
      Err(error) => {
        errors.push(field_error(font_type, "script", error));
        None
      },
    },
  };

  let ot_language_tag = match pre_font_config.ot_language.as_deref() {
    None => None,
    Some(value) => match tag::parse_ot_language_tag(value) {
      Ok(bytes) => Some(bytes),
      Err(error) => {
        errors.push(field_error(font_type, "ot_language", error));
        None
      },
    },
  };

  let direction = match pre_font_config.direction.as_deref() {
    None => None,
    Some(value) => match value.parse::<TextDirection>() {
      Ok(direction) => Some(direction),
      Err(error) => {
        errors.push(field_error(font_type, "direction", error));
        None
      },
    },
  };

  // 軸: 入力が Some なら（空配列でも）Some を維持する（旧挙動）。不正な軸名は集約。
  let variation_axes = pre_font_config.variation_axes.as_deref().map(|axes| {
    axes
      .iter()
      .filter_map(|axis| match tag::parse_opentype_tag(&axis.name) {
        Ok(name) => Some(VariationAxis {
          name,
          value: axis.value,
        }),
        Err(error) => {
          errors.push(field_error(font_type, "variation_axes", error));
          None
        },
      })
      .collect::<Vec<_>>()
  });

  // フィーチャー: 空配列は None 扱い（旧挙動）。不正なタグは集約。
  let features = pre_font_config.features.as_deref().and_then(|feats| {
    let converted: Vec<Feature> = feats
      .iter()
      .filter_map(|feature| match tag::parse_opentype_tag(&feature.tag) {
        Ok(tag) => Some(Feature {
          tag,
          value: feature.value,
        }),
        Err(error) => {
          errors.push(field_error(font_type, "features", error));
          None
        },
      })
      .collect();
    (!converted.is_empty()).then_some(converted)
  });

  let language = build_language_string(pre_font_config.language.as_deref(), pre_font_config.ot_language.as_deref());

  if !errors.is_empty() {
    return Err(errors);
  }
  return Ok(FontValues {
    font_name: pre_font_config.font_name.clone(),
    font_index: pre_font_config.font_index,
    variation_axes,
    script,
    language,
    ot_language_tag,
    direction,
    features,
  });
}

/// フォント種別とフィールド名から、タグ・書字方向の不正を表す [`ValidationError::Field`] を作ります。
///
/// `path` は `garde` のフィールドパス（例: `"font_configs.serif.script"`）と同じ書式に揃え、
/// `message` には [`crate::tag::TagError`] / [`processed_config::TextDirectionParseError`] の
/// 説明文をそのまま使用します。
fn field_error(font_type: FontType, field: &str, error: impl std::fmt::Display) -> ValidationError {
  return ValidationError::Field {
    path: format!("font_configs.{}.{field}", font_type.as_toml_key()),
    message: error.to_string(),
  };
}

/// BCP 47 言語タグと OT 言語タグから、harfrust の [`Language::from_str`] に渡す最終 BCP 47 文字列を構築します。
///
/// `ot_language` が指定されている場合は、ベースの BCP 47（未指定なら `"und"`）の末尾に
/// `-x-hbot<TAG>` 予約サブタグを連結します。harfrust 側でこのサブタグを解釈し、
/// GSUB/GPOS の言語タグ導出に直接使用します（[`tag.rs::parse_private_use_subtag`] 経由）。
///
/// [`Language::from_str`]: https://docs.rs/harfrust/latest/harfrust/struct.Language.html
fn build_language_string(language: Option<&str>, ot_language: Option<&str>) -> Option<String> {
  match (language, ot_language) {
    (None, None) => return None,
    (Some(lang), None) => return Some(lang.to_string()),
    (None, Some(ot_lang)) => return Some(format!("und-x-hbot{ot_lang}")),
    (Some(lang), Some(ot_lang)) => return Some(format!("{lang}-x-hbot{ot_lang}")),
  }
}

/// 出力ディレクトリの絶対パスを決定します（I/O なし・純粋）。
///
/// `output_dir` が絶対パスならそのまま、相対パスなら `current_dir` に結合、
/// `None` なら `current_dir` 自体を出力先とします。実ディレクトリの作成・正規化は
/// 行いません（[`build_output_dir`] の責務）。
fn resolve_output_dir_path(current_dir: &Path, output_dir: Option<&Path>) -> PathBuf {
  match output_dir {
    Some(path) if path.is_absolute() => return path.to_path_buf(),
    Some(path) => return current_dir.join(path),
    None => return current_dir.to_path_buf(),
  }
}

/// 書き込み I/O フェーズ: 出力ディレクトリを作成・正規化し、絶対パスを返します。
///
/// パスの決定は純粋関数 [`resolve_output_dir_path`] に委譲し、本関数は `mkdir`
/// （`create_dir_all`）と `canonicalize` の I/O だけを担います。
/// `output_dir` が `None` の場合は `current_dir` をそのまま出力先とします（カレント直下に出力）。
/// 実際の PDF パス（`{output_dir}/{name}.pdf`）は [`OutputConfig::pdf_path`] が組み立てます。
/// エラーは [`ValidationError`] として返し、呼び出し側で他の検証エラーと同じ
/// [`ReadConfigError::MultipleValidationErrors`] に集約します。
fn build_output_dir(current_dir: &Path, output_dir: Option<&Path>) -> Result<PathBuf, ValidationError> {
  let output_dir_path = resolve_output_dir_path(current_dir, output_dir);
  fs::create_dir_all(&output_dir_path).map_err(|source| ValidationError::CreateOutputDir {
    path: output_dir_path.display().to_string(),
    source,
  })?;
  let canonical = output_dir_path.canonicalize().map_err(|source| ValidationError::CanonicalizeOutputDir {
    path: output_dir_path.display().to_string(),
    source,
  })?;
  return Ok(canonical);
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use super::{
    ReadConfigError, ValidationError, build_language_string, parse_config, resolve_output_dir_path, validate_values,
  };
  use crate::test_support::{make_font_sections, valid_output_section, valid_pdf_section};

  /// `parse_config` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  #[test]
  fn resolve_output_dir_path_keeps_absolute_path_as_is() {
    // Arrange
    let current_dir = Path::new("/home/user/project");
    let output_dir = PathBuf::from("/var/out");

    // Act
    let resolved = resolve_output_dir_path(current_dir, Some(&output_dir));

    // Assert: 絶対パスは current_dir を無視してそのまま返る
    assert_eq!(resolved, PathBuf::from("/var/out"));
  }

  #[test]
  fn resolve_output_dir_path_joins_relative_path_to_current_dir() {
    // Arrange
    let current_dir = Path::new("/home/user/project");
    let output_dir = PathBuf::from("build/out");

    // Act
    let resolved = resolve_output_dir_path(current_dir, Some(&output_dir));

    // Assert: 相対パスは current_dir に結合される
    assert_eq!(resolved, PathBuf::from("/home/user/project/build/out"));
  }

  #[test]
  fn resolve_output_dir_path_uses_current_dir_when_none() {
    // Arrange
    let current_dir = Path::new("/home/user/project");

    // Act
    let resolved = resolve_output_dir_path(current_dir, None);

    // Assert: 省略時は current_dir 自体が出力先になる
    assert_eq!(resolved, PathBuf::from("/home/user/project"));
  }

  #[test]
  fn parse_config_fails_on_invalid_toml_syntax() {
    // Arrange / Act
    let result = parse_config("name = \nthis is not valid toml", dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadConfigError::ParseToml { .. })));
  }

  #[test]
  fn parse_toml_error_records_span_and_suppresses_inner_display_input() {
    // Arrange: クォート抜けの不正な TOML
    let invalid_toml = "name = bad\n";

    // Act
    let err = parse_config(invalid_toml, dummy_source()).unwrap_err();

    // Assert
    let ReadConfigError::ParseToml {
      src: _,
      span,
      source,
    } = err
    else {
      panic!("expected ParseToml variant");
    };
    // span が文字列内の位置を指していること（先頭固定でない / 何らかの正の幅を持つ）
    assert!(span.offset() > 0 || !span.is_empty(), "span must point at the syntax issue, got {span:?}");
    // set_input(None) により toml の Display 側の自前 line/column スニペットが消えている
    let rendered = source.to_string();
    assert!(
      !rendered.contains("TOML parse error at line"),
      "set_input(None) should suppress toml's built-in snippet, but got: {rendered}"
    );
  }

  #[test]
  fn validate_values_fails_on_negative_margin() {
    // Arrange
    let toml = format!(
      "{}[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
       margin_top = \"-10pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n{}",
      valid_output_section("test", "out"),
      make_font_sections("dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path.contains("margin_top")
    )));
  }

  #[test]
  fn validate_values_fails_on_margin_sum_exceeding_dimension() {
    // Arrange: vertical margin sum (60+60=120) >= height (100)
    let toml = format!(
      "{}[pdf]\nheight = \"100pt\"\nwidth = \"595pt\"\n\
       margin_top = \"60pt\"\nmargin_bottom = \"60pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n{}",
      valid_output_section("test", "out"),
      make_font_sections("dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path == "pdf" && message.contains("vertical")
    )));
  }

  #[test]
  fn validate_values_fails_on_duplicate_font_names_with_font_type_in_path() {
    // Arrange: serif_bold が serif と同じ font_name を使う
    let sections = make_font_sections("dummy.ttf").replace(
      "[font_configs.serif_bold]\nfont_name = \"font_serif_bold\"",
      "[font_configs.serif_bold]\nfont_name = \"font_serif\"",
    );
    let toml = format!("{}{}{sections}", valid_output_section("test", "out"), valid_pdf_section());
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    let dup_path = errors
      .iter()
      .find_map(|error| match error {
        ValidationError::Field { path, message } if message.contains("重複") => Some(path.as_str()),
        _ => None,
      })
      .expect("expected duplicate font name error");
    assert_eq!(dup_path, "font_configs.serif_bold");
  }

  #[test]
  fn parse_config_fails_on_legacy_top_level_name() {
    // 旧構造（トップレベル `name`）の TOML は新構造のスキーマと一致せず、
    // toml デシリアライズが失敗するため、利用者は構造変更に気づける。
    let toml = format!(
      "name = \"test\"\n\n[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
       margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n{}",
      make_font_sections("dummy.ttf"),
    );
    let result = parse_config(&toml, dummy_source());
    assert!(matches!(result, Err(ReadConfigError::ParseToml { .. })));
  }

  #[test]
  fn validate_values_fails_on_empty_sources() {
    // `sources = []` は禁止
    let toml = format!(
      "sources = []\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      make_font_sections("dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();
    let errors = validate_values(&pre).unwrap_err();
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "sources"
    )));
  }

  #[test]
  fn validate_values_fails_on_omitted_sources() {
    // `sources` キー自体を省略しても、空配列扱いで同じエラーになる
    let toml =
      format!("{}{}{}", valid_output_section("test", "out"), valid_pdf_section(), make_font_sections("dummy.ttf"));
    let pre = parse_config(&toml, dummy_source()).unwrap();
    let errors = validate_values(&pre).unwrap_err();
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "sources"
    )));
  }

  #[test]
  fn validate_values_fails_on_empty_output_dir() {
    // Arrange: output_dir = "" は明示的に拒否
    let toml =
      format!("{}{}{}", valid_output_section("test", ""), valid_pdf_section(), make_font_sections("dummy.ttf"));
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "output.output_dir"
    )));
  }

  /// 指定の `[font_configs.serif]` 追加行で TOML を組み、`validate_values` を通した結果を返します。
  ///
  /// `validate_values` は I/O を行わないので、`sources` のパスは実在しなくても構わない
  /// （`resolve` でのみ canonicalize される）。空配列だけ避ければ良い。
  fn run_validate_with_serif_extra(extra_lines: &str) -> Result<(), Vec<ValidationError>> {
    let toml = format!(
      "sources = [\"dummy.sei\"]\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      crate::test_support::font_sections_with_serif_extra("dummy.ttf", extra_lines),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();
    return validate_values(&pre);
  }

  #[test]
  fn validate_values_accepts_valid_bcp47_languages() {
    // Arrange / Act / Assert: 主要な BCP 47 形式が通る
    for lang in ["ja", "en-US", "zh-Hant", "zh-Hans-CN", "und"] {
      let extra = format!("language = \"{lang}\"");
      assert!(run_validate_with_serif_extra(&extra).is_ok(), "expected '{lang}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_bcp47_language() {
    // Arrange / Act
    let errors = run_validate_with_serif_extra("language = \"!!\"").unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path.contains("language") && message.contains("BCP 47")
    )));
  }

  #[test]
  fn validate_values_rejects_reserved_private_use_in_language() {
    // Arrange / Act: ユーザが `-x-hbsc` / `-x-hbot` を直接書くのは禁止
    for forbidden in ["en-x-hbsclatn", "ja-x-hbotJAN"] {
      let extra = format!("language = \"{forbidden}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ValidationError::Field { path, message }
            if path.contains("language") && (message.contains("-x-hbsc") || message.contains("-x-hbot"))
        )),
        "expected '{forbidden}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_accepts_structurally_valid_ot_script_tags() {
    // Arrange / Act / Assert: structural チェックは「4 文字 ASCII アルファベット」のみ。
    // OT 慣例外（"Hani", "Latn", "dflt" 等）も hard error にはしない。フォント実態との突合せは
    // font::validate_font が GSUB/GPOS の ScriptList をバイト完全一致 lookup で報告する。
    for script in [
      "latn", "kana", "hani", "DFLT", "Hani", "Latn", "LATN", "dflt", "Dflt",
    ] {
      let extra = format!("script = \"{script}\"");
      assert!(
        run_validate_with_serif_extra(&extra).is_ok(),
        "expected script='{script}' to pass structural validation"
      );
    }
  }

  #[test]
  fn validate_values_rejects_structurally_invalid_ot_script_tag() {
    // Arrange / Act / Assert: 長さ違い、digit、非 ASCII alphabetic は hard error
    for script in ["kan", "kanaa", "kan1", "ka一"] {
      let extra = format!("script = \"{script}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ValidationError::Field { path, .. } if path.contains("script")
        )),
        "expected script='{script}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_accepts_valid_ot_language_with_script() {
    // Arrange / Act / Assert: 3-4 文字 ASCII alphanumeric、script 必須
    for ot_lang in ["JAN", "ENG", "DEU", "ZHS"] {
      let extra = format!("script = \"latn\"\not_language = \"{ot_lang}\"");
      assert!(run_validate_with_serif_extra(&extra).is_ok(), "expected ot_language='{ot_lang}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_ot_language_tag() {
    // Arrange / Act / Assert: 長さ違い、非 alphanumeric
    for ot_lang in ["JA", "JAPAN", "J!N"] {
      let extra = format!("script = \"latn\"\not_language = \"{ot_lang}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ValidationError::Field { path, .. } if path.contains("ot_language")
        )),
        "expected ot_language='{ot_lang}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_rejects_ot_language_without_script() {
    // Arrange: script を指定せず ot_language のみ指定
    let extra = "ot_language = \"JAN\"";

    // Act
    let errors = run_validate_with_serif_extra(extra).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message }
        if path == "font_configs.serif" && message.contains("ot_language") && message.contains("script")
    )));
  }

  #[test]
  fn build_language_string_handles_all_combinations() {
    // Arrange / Act / Assert
    assert_eq!(build_language_string(None, None), None);
    assert_eq!(build_language_string(Some("ja"), None), Some("ja".to_string()));
    assert_eq!(build_language_string(None, Some("JAN")), Some("und-x-hbotJAN".to_string()));
    assert_eq!(build_language_string(Some("en-US"), Some("ENG")), Some("en-US-x-hbotENG".to_string()));
  }

  #[test]
  fn validate_values_accepts_all_valid_directions() {
    // Arrange / Act / Assert: 4 種類のハイフン区切り長形が通る
    for direction in [
      "left-to-right",
      "right-to-left",
      "top-to-bottom",
      "bottom-to-top",
    ] {
      let extra = format!("direction = \"{direction}\"");
      assert!(run_validate_with_serif_extra(&extra).is_ok(), "expected direction='{direction}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_direction() {
    // Arrange / Act / Assert: 短縮形・大文字・スペース区切り・空文字・誤綴りは拒否
    for direction in [
      "ltr",
      "LTR",
      "left to right",
      "",
      "leftToRight",
      "horizontal",
    ] {
      let extra = format!("direction = \"{direction}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ValidationError::Field { path, .. } if path.contains("direction")
        )),
        "expected direction='{direction}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_accepts_valid_document_language() {
    // Arrange / Act / Assert: 主要な BCP 47 形式が document.language で通る
    for lang in ["ja", "en-US", "zh-Hant", "und"] {
      let toml = format!(
        "sources = [\"dummy.sei\"]\n\n[document]\nlanguage = \"{lang}\"\n\n{}{}{}",
        valid_output_section("test", "out"),
        valid_pdf_section(),
        make_font_sections("dummy.ttf"),
      );
      let pre = parse_config(&toml, dummy_source()).unwrap();
      assert!(validate_values(&pre).is_ok(), "expected document.language='{lang}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_document_language() {
    // Arrange
    let toml = format!(
      "sources = [\"dummy.sei\"]\n\n[document]\nlanguage = \"!!\"\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      make_font_sections("dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path.contains("language") && message.contains("BCP 47")
    )));
  }

  #[test]
  fn validate_values_rejects_empty_keyword_entry() {
    // Arrange: keywords の途中に空文字列を含める
    let toml = format!(
      "sources = [\"dummy.sei\"]\n\n[document]\nkeywords = [\"foo\", \"\", \"bar\"]\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      make_font_sections("dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path.contains("keywords") && message.contains("空")
    )));
  }
}
