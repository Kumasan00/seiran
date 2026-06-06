//! TOML 設定ファイルのパース・検証・変換モジュール
//!
//! PDF 生成に必要な設定情報を TOML から読み込み、`garde` による宣言的バリデーション、
//! ファイルパスの解決、型変換を実行して構造化設定データを構築します。

use std::{
  fs,
  path::{Path, PathBuf},
};

use garde::Validate;
use miette::Diagnostic;
use thiserror::Error;
use tracing::{info, warn};
use types::FontType;

mod pre_config;
use pre_config::{PreConfig, PreFontConfig};
mod processed_config;

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
  #[error("設定ファイルの TOML 解析に失敗しました: {path}")]
  #[diagnostic(code(config::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    path: String,
    #[source]
    source: toml::de::Error,
  },
  /// 出力ディレクトリの作成失敗
  #[error("出力ディレクトリを作成できませんでした: {path}")]
  #[diagnostic(
    code(config::create_output_dir),
    help("親ディレクトリが存在し、書き込み権限があることを確認してください。")
  )]
  CreateOutputDir {
    path: String,
    #[source]
    source: std::io::Error,
  },
  /// 出力ディレクトリのパス正規化失敗
  #[error("出力ディレクトリのパスを正規化できませんでした: {path}")]
  #[diagnostic(code(config::canonicalize_output_dir), help("指定したディレクトリが存在するか確認してください。"))]
  CanonicalizeOutputDir {
    path: String,
    #[source]
    source: std::io::Error,
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
}

/// 指定パスから設定ファイルを読み込みます。
///
/// [`parse_config`] → [`resolve`] を連結する利便ラッパで、生産コードから使う想定です。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション・出力パス構築の失敗時にエラーを返します。
pub fn read_config(config_path: &Path) -> Result<Config, ReadConfigError> {
  info!(config_path = %config_path.display(), "設定ファイルの読み込みを開始します");
  let config_content = fs::read(config_path).map_err(|source| ReadConfigError::ReadFile {
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

/// TOML バイト列を [`PreConfig`] にパースします（I/O なし）。
///
/// `source_path` はエラー報告に使う表示用パスで、ファイルシステムへのアクセスには使われません。
/// 値検証は行いません。検証は [`validate_values`] または [`resolve`] で実行します。
///
/// # Errors
///
/// TOML の構文が不正な場合に [`ReadConfigError::ParseToml`] を返します。
fn parse_config(bytes: &[u8], source_path: &Path) -> Result<PreConfig, ReadConfigError> {
  return toml::from_slice(bytes).map_err(|source| ReadConfigError::ParseToml {
    path: source_path.display().to_string(),
    source,
  });
}

/// [`PreConfig`] からパス解決・出力ディレクトリ作成を行い [`Config`] を構築します。
///
/// 値検証も内部で再実行し、値違反とパス解決エラーを 1 回の
/// [`ReadConfigError::MultipleValidationErrors`] に集約します。
///
/// # Errors
///
/// - 値検証またはパス解決に違反があった場合は [`ReadConfigError::MultipleValidationErrors`]
/// - 出力ディレクトリ作成・正規化に失敗した場合は [`ReadConfigError::CreateOutputDir`] または
///   [`ReadConfigError::CanonicalizeOutputDir`]
fn resolve(pre: PreConfig, current_dir: &Path) -> Result<Config, ReadConfigError> {
  // 値検証エラーとパス解決エラーを単一の `errors` に集約して 1 度に報告する。
  let mut errors: Vec<ValidationError> = match validate_values(&pre) {
    Ok(()) => Vec::new(),
    Err(value_errors) => value_errors,
  };

  let style_path = canonicalize_or_record(pre.style_path.as_deref(), &mut errors, |path, source| {
    ValidationError::StylePathResolution { path, source }
  });
  let references_path = canonicalize_or_record(pre.references_path.as_deref(), &mut errors, |path, source| {
    ValidationError::ReferencesPathResolution { path, source }
  });

  let mut font_configs_vec: Vec<FontConfig> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    match to_font_config(font_type, pre.font_configs.get(font_type)) {
      Ok(font_config) => font_configs_vec.push(font_config),
      Err(error) => errors.push(error),
    }
  }

  // sources パスの正規化を試行（失敗はバリデーションエラーに集約）
  let resolved_sources = canonicalize_sources(&pre.sources, current_dir, &mut errors);

  if !errors.is_empty() {
    return Err(ReadConfigError::MultipleValidationErrors { errors });
  }

  // 副作用フェーズ: 検証通過後にディレクトリ作成や正規化を実行する
  let PreConfig {
    document: pre_document,
    output: pre_output,
    pdf: pre_pdf_config,
    font_configs: _,
    sources: _,
    style_path: _,
    references_path: _,
  } = pre;

  let output_dir = build_output_dir(current_dir, pre_output.output_dir.as_deref())?;
  let font_configs = FontConfigs::from_all(font_configs_vec);

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
    sources: resolved_sources,
    style_path,
    references_path,
  });
}

/// [`PreConfig`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証、上下／左右の余白合計、19 フォント種別の `font_name` 重複を
/// すべて 1 度に集約して返します。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(pre: &PreConfig) -> Result<(), Vec<ValidationError>> {
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
  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
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

/// `PreFontConfig` を `FontConfig` に変換します。
fn to_font_config(font_type: FontType, pre_font_config: &PreFontConfig) -> Result<FontConfig, ValidationError> {
  let font_path = pre_font_config.font_path.canonicalize().map_err(|source| ValidationError::FontPathResolution {
    font_type,
    path: pre_font_config.font_path.display().to_string(),
    source,
  })?;

  let script = pre_font_config.script.as_deref().map(four_byte_tag);
  let ot_language_tag = pre_font_config.ot_language.as_deref().map(normalize_ot_language_tag);
  let language = build_language_string(pre_font_config.language.as_deref(), pre_font_config.ot_language.as_deref());
  let direction = pre_font_config.direction.as_deref().map(parse_text_direction);
  let features = pre_font_config.features.as_deref().and_then(|fs| {
    let v: Vec<Feature> = fs
      .iter()
      .map(|f| Feature {
        tag: four_byte_tag(&f.tag),
        value: f.value,
      })
      .collect();
    (!v.is_empty()).then_some(v)
  });
  let variation_axes = pre_font_config.variation_axes.as_deref().map(|axes| {
    axes
      .iter()
      .map(|a| VariationAxis {
        name: four_byte_tag(&a.name),
        value: a.value,
      })
      .collect::<Vec<_>>()
  });

  return Ok(FontConfig {
    font_name: pre_font_config.font_name.clone(),
    font_path,
    font_index: pre_font_config.font_index,
    variation_axes,
    script,
    language,
    ot_language_tag,
    direction,
    features,
  });
}

/// 4 バイト ASCII 文字列を `[u8; 4]` に変換します（カスタムバリデーターで長さと ASCII を検証済み）。
#[allow(clippy::unwrap_used)]
fn four_byte_tag(s: &str) -> [u8; 4] { return s.as_bytes().try_into().unwrap(); }

/// 3 または 4 文字の OT 言語タグを `[u8; 4]` に正規化します。
///
/// 大文字化し、4 バイト未満の場合は末尾を空白でパディングします（OpenType 言語システムタグの慣習）。
/// `font` クレートの `validate_font` で GSUB/GPOS 言語サブテーブルの参照に使用します。
fn normalize_ot_language_tag(s: &str) -> [u8; 4] {
  let mut bytes = [b' '; 4];
  for (i, b) in s.as_bytes().iter().enumerate().take(4) {
    bytes[i] = b.to_ascii_uppercase();
  }
  return bytes;
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

/// `validate_direction` で検証済みの direction 文字列を [`TextDirection`] に変換します。
///
/// validator が hard error を出す形で値域を 4 つに制限しているため、ここに想定外の値は
/// 到達しません（`unreachable!` で明示）。
fn parse_text_direction(s: &str) -> TextDirection {
  return match s {
    "left-to-right" => TextDirection::LeftToRight,
    "right-to-left" => TextDirection::RightToLeft,
    "top-to-bottom" => TextDirection::TopToBottom,
    "bottom-to-top" => TextDirection::BottomToTop,
    other => unreachable!("validate_direction で検証済みのはずだが '{other}' が到達した"),
  };
}

/// 出力ディレクトリを作成・正規化し、絶対パスを返します。
///
/// `output_dir` が `None` の場合は `current_dir` をそのまま出力先とします（カレント直下に出力）。
/// 実際の PDF パス（`{output_dir}/{name}.pdf`）は [`OutputConfig::pdf_path`] が組み立てます。
fn build_output_dir(current_dir: &Path, output_dir: Option<&Path>) -> Result<PathBuf, ReadConfigError> {
  let output_dir_path = match output_dir {
    Some(path) if path.is_absolute() => path.to_path_buf(),
    Some(path) => current_dir.join(path),
    None => current_dir.to_path_buf(),
  };
  fs::create_dir_all(&output_dir_path).map_err(|source| ReadConfigError::CreateOutputDir {
    path: output_dir_path.display().to_string(),
    source,
  })?;
  let canonical = output_dir_path.canonicalize().map_err(|source| ReadConfigError::CanonicalizeOutputDir {
    path: output_dir_path.display().to_string(),
    source,
  })?;
  return Ok(canonical);
}

#[cfg(test)]
mod tests {
  use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
  };

  use tempfile::TempDir;
  use types::FontType;

  use super::{
    Config, ReadConfigError, TextDirection, ValidationError, build_language_string, normalize_ot_language_tag,
    parse_config, read_config, validate_values,
  };

  /// 19 フォント種別すべての設定セクションを生成するヘルパー。
  fn make_font_sections(font_path: &str) -> String {
    const SECTION_NAMES: [&str; 19] = [
      "serif",
      "serif_bold",
      "serif_italic",
      "serif_bold_italic",
      "sans_serif",
      "sans_serif_bold",
      "sans_serif_italic",
      "sans_serif_bold_italic",
      "monospace",
      "monospace_bold",
      "monospace_italic",
      "monospace_bold_italic",
      "math",
      "japanese_serif",
      "japanese_serif_bold",
      "japanese_sans_serif",
      "japanese_sans_serif_bold",
      "japanese_monospace",
      "japanese_monospace_bold",
    ];
    let mut out = String::new();
    for name in SECTION_NAMES {
      write!(out, "[font_configs.{name}]\nfont_name = \"font_{name}\"\nfont_path = \"{font_path}\"\n\n").unwrap();
    }
    return out;
  }

  /// 既定の `[output]` セクション（妥当な値）を生成します。
  fn valid_output_section(name: &str, output_dir: &str) -> String {
    return format!("[output]\nname = \"{name}\"\noutput_dir = \"{output_dir}\"\n\n");
  }

  /// 既定の `[pdf]` セクション（妥当な値）を生成します。
  fn valid_pdf_section() -> String {
    return "[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
            margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n"
      .to_string();
  }

  /// `parse_config` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  /// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します。
  ///
  /// `build_toml(font_path, output_dir, source_path)` の各引数は絶対パス文字列で、
  /// テスト側はこれらを TOML テキストの組み立てに使う。
  fn setup_config(build_toml: impl FnOnce(&str, &str, &str) -> String) -> (TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().unwrap();
    let font_path = tempdir.path().join("dummy.ttf");
    std::fs::write(&font_path, b"").unwrap();
    let source_path = tempdir.path().join("source.sei");
    std::fs::write(&source_path, b"").unwrap();
    let output_dir = tempdir.path().join("output");
    let config_path = tempdir.path().join("config.toml");
    let toml_text =
      build_toml(font_path.to_str().unwrap(), output_dir.to_str().unwrap(), source_path.to_str().unwrap());
    std::fs::write(&config_path, toml_text).unwrap();
    return (tempdir, config_path);
  }

  #[test]
  fn parse_config_fails_on_invalid_toml_syntax() {
    // Arrange / Act
    let result = parse_config(b"name = \nthis is not valid toml", dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadConfigError::ParseToml { .. })));
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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();

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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();

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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();

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
    assert!(dup_path.contains("SerifBold"), "path should contain FontType, got: {dup_path}");
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
    let result = parse_config(toml.as_bytes(), dummy_source());
    assert!(matches!(result, Err(ReadConfigError::ParseToml { .. })));
  }

  #[test]
  fn read_config_succeeds_with_valid_config() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      format!(
        "sources = [\"{source_path}\"]\n\n[document]\ntitle = \"Test Doc\"\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert
    assert_eq!(config.output.name, "test_doc");
    assert_eq!(config.document.title.as_deref(), Some("Test Doc"));
    assert_eq!(config.sources.len(), 1);
    assert_eq!(config.font_configs.iter().count(), 19);
    assert_eq!(config.font_configs.get(FontType::Serif).font_name, "font_serif");
  }

  #[test]
  fn read_config_fails_on_nonexistent_font_path() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|_font_path, output_dir, source_path| {
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test", output_dir),
        valid_pdf_section(),
        make_font_sections("/nonexistent/path/to/font.ttf"),
      )
    });

    // Act
    let result = read_config(&config_path);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().all(|error| matches!(error, ValidationError::FontPathResolution { .. })));
    assert_eq!(errors.len(), 19);
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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();
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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();
    let errors = validate_values(&pre).unwrap_err();
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "sources"
    )));
  }

  #[test]
  fn read_config_fails_on_nonexistent_source_path() {
    // Arrange: 存在しない source パスを指定
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, _source_path| {
      format!(
        "sources = [\"/nonexistent/source.sei\"]\n\n{}{}{}",
        valid_output_section("test", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      )
    });

    // Act
    let result = read_config(&config_path);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().any(|error| matches!(error, ValidationError::SourcePathResolution { .. })));
  }

  #[test]
  fn validate_values_fails_on_empty_output_dir() {
    // Arrange: output_dir = "" は明示的に拒否
    let toml =
      format!("{}{}{}", valid_output_section("test", ""), valid_pdf_section(), make_font_sections("dummy.ttf"));
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path == "output.output_dir"
    )));
  }

  #[test]
  fn read_config_uses_current_dir_when_output_dir_omitted() {
    // Arrange: TOML から output_dir を省略。CWD を変えると並列テストに影響するため、
    // 現在の CWD を期待値として観察する。
    let (_tempdir, config_path) = setup_config(|font_path, _output_dir, source_path| {
      format!(
        "sources = [\"{source_path}\"]\n\n[output]\nname = \"out\"\n\n{}{}",
        valid_pdf_section(),
        make_font_sections(font_path),
      )
    });
    let expected_output_dir = std::env::current_dir().unwrap().canonicalize().unwrap();

    // Act
    let config = read_config(&config_path).unwrap();

    // Assert: 出力先は省略時の CWD と一致する
    assert_eq!(config.output.output_dir, expected_output_dir);
    assert_eq!(config.output.pdf_path(), expected_output_dir.join("out.pdf"));
  }

  /// `[font_configs.serif]` セクションに任意のフィールド追加行を差し込んだ TOML を生成します。
  ///
  /// `extra_lines` には `font_name` / `font_path` 以外のフィールド（例: `language = "ja-JP"`）を
  /// 改行区切りで指定します。
  fn font_sections_with_serif_extra(font_path: &str, extra_lines: &str) -> String {
    let base = make_font_sections(font_path);
    let needle = "[font_configs.serif]\nfont_name = \"font_serif\"\nfont_path = \"";
    let injected_marker = format!("[font_configs.serif]\nfont_name = \"font_serif\"\n{extra_lines}\nfont_path = \"");
    return base.replace(needle, &injected_marker);
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
      font_sections_with_serif_extra("dummy.ttf", extra_lines),
    );
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();
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
  fn read_config_preserves_user_script_tag_case() {
    // 構造的に妥当な OT 慣例外 ("Latn" 等) は read_config 段階では受け付け、ユーザ指定の case が
    // そのまま保存される。フォントとの突合せは font::validate_font が担当する。
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "script = \"Latn\"";
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      )
    });

    let config: Config = read_config(&config_path).unwrap();

    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.script, Some(*b"Latn"));
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
        if path.contains("Serif") && message.contains("ot_language") && message.contains("script")
    )));
  }

  #[test]
  fn read_config_builds_language_string_with_ot_language_suffix() {
    // Arrange: BCP 47 + script + ot_language を指定
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "language = \"ja\"\nscript = \"kana\"\not_language = \"JAN\"";
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert: final language string is `ja-x-hbotJAN`、script/ot_language_tag は正規化済み
    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.language.as_deref(), Some("ja-x-hbotJAN"));
    assert_eq!(serif.script, Some(*b"kana"));
    assert_eq!(serif.ot_language_tag, Some(*b"JAN "));
  }

  #[test]
  fn read_config_builds_language_string_with_und_base_when_only_ot_language() {
    // Arrange: language 未指定で script + ot_language のみ → `und-x-hbot<TAG>` が生成される
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "script = \"latn\"\not_language = \"ENG\"";
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert
    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.language.as_deref(), Some("und-x-hbotENG"));
    assert_eq!(serif.ot_language_tag, Some(*b"ENG "));
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
  fn normalize_ot_language_tag_uppercases_and_pads_to_four_bytes() {
    // Arrange / Act / Assert: 3 文字は末尾スペース、小文字は大文字化
    assert_eq!(normalize_ot_language_tag("JAN"), *b"JAN ");
    assert_eq!(normalize_ot_language_tag("eng"), *b"ENG ");
    assert_eq!(normalize_ot_language_tag("DEUT"), *b"DEUT");
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
  fn read_config_preserves_user_direction() {
    // Arrange: right-to-left を指定
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "direction = \"right-to-left\"";
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert
    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.direction, Some(TextDirection::RightToLeft));
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
      let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();
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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();

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
    let pre = parse_config(toml.as_bytes(), dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path.contains("keywords") && message.contains("空")
    )));
  }

  #[test]
  fn read_config_preserves_document_language_and_keywords() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      format!(
        "sources = [\"{source_path}\"]\n\n[document]\nlanguage = \"ja\"\nkeywords = [\"組版\", \"PDF\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert
    assert_eq!(config.document.language.as_deref(), Some("ja"));
    assert_eq!(config.document.keywords.as_deref(), Some(&["組版".to_string(), "PDF".to_string()][..]));
  }

  #[test]
  fn read_config_keeps_document_language_and_keywords_none_when_omitted() {
    // Arrange: language / keywords を省略
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert
    assert_eq!(config.document.language, None);
    assert_eq!(config.document.keywords, None);
  }

  #[test]
  fn read_config_keeps_direction_none_when_omitted() {
    // Arrange: direction を指定せず最小構成で読み込む
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      )
    });

    // Act
    let config: Config = read_config(&config_path).unwrap();

    // Assert: 19 フォントすべて direction = None
    for font_type in FontType::ALL {
      assert_eq!(config.font_configs.get(font_type).direction, None, "{font_type:?}");
    }
  }
}
