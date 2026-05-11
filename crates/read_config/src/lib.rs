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
use tracing::info;
use types::FontType;

mod pre_config;
use pre_config::{PreConfig, PreFontConfig};
mod processed_config;

pub use processed_config::{
  Config, DocumentConfig, Feature, FontConfig, FontConfigs, Margin, OutputConfig, PdfConfig, VariationAxis,
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

  let output_dir = build_output_dir(current_dir, &pre_output.output_dir)?;
  let font_configs = FontConfigs::from_all(font_configs_vec);

  return Ok(Config {
    document: DocumentConfig {
      title: pre_document.title,
      author: pre_document.author,
      date: pre_document.date,
      subject: pre_document.subject,
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

/// 各 source パスを `canonicalize` し、失敗時はエラーを `errors` に追加する。
fn canonicalize_sources(sources: &[PathBuf], current_dir: &Path, errors: &mut Vec<ValidationError>) -> Vec<PathBuf> {
  let mut resolved = Vec::with_capacity(sources.len());
  for source_path in sources {
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

/// `PreFontConfig` を `FontConfig` に変換します。
fn to_font_config(font_type: FontType, pre_font_config: &PreFontConfig) -> Result<FontConfig, ValidationError> {
  let font_path = pre_font_config.font_path.canonicalize().map_err(|source| ValidationError::FontPathResolution {
    font_type,
    path: pre_font_config.font_path.display().to_string(),
    source,
  })?;

  let script = pre_font_config.script.as_deref().map(four_byte_tag);
  let language = pre_font_config.language.as_deref().map(language_tag);
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
    font_index: pre_font_config.font_index.unwrap_or(0),
    variation_axes,
    script,
    language,
    features,
  });
}

/// 4 バイト ASCII 文字列を `[u8; 4]` に変換します（カスタムバリデーターで長さと ASCII を検証済み）。
#[allow(clippy::unwrap_used)]
fn four_byte_tag(s: &str) -> [u8; 4] { return s.as_bytes().try_into().unwrap(); }

/// 3 or 4 文字の言語タグを `[u8; 4]` に変換します（3 文字時は末尾スペース）。
fn language_tag(s: &str) -> [u8; 4] {
  let b = s.as_bytes();
  if b.len() == 4 {
    return [b[0], b[1], b[2], b[3]];
  }
  return [b[0], b[1], b[2], b' '];
}

/// 出力ディレクトリを作成・正規化し、絶対パスを返します。
///
/// 実際の PDF パス（`{output_dir}/{name}.pdf`）は [`OutputConfig::pdf_path`] が組み立てます。
fn build_output_dir(current_dir: &Path, output_dir: &Path) -> Result<PathBuf, ReadConfigError> {
  let output_dir_path = if output_dir.is_absolute() {
    output_dir.to_path_buf()
  } else {
    current_dir.join(output_dir)
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

  use super::{Config, ReadConfigError, ValidationError, parse_config, read_config, validate_values};

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
    return "[pdf]\nheight = 842.0\nwidth = 595.0\n\
            margin_top = 50.0\nmargin_bottom = 50.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n"
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
    let source_path = tempdir.path().join("source.txt");
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
      "{}[pdf]\nheight = 842.0\nwidth = 595.0\n\
       margin_top = -10.0\nmargin_bottom = 50.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n{}",
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
      "{}[pdf]\nheight = 100.0\nwidth = 595.0\n\
       margin_top = 60.0\nmargin_bottom = 60.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n{}",
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
      "name = \"test\"\n\n[pdf]\nheight = 842.0\nwidth = 595.0\n\
       margin_top = 50.0\nmargin_bottom = 50.0\nmargin_left = 50.0\nmargin_right = 50.0\n\n{}",
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
        "sources = [\"/nonexistent/source.txt\"]\n\n{}{}{}",
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
}
