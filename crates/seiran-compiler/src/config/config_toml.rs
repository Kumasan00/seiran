//! TOML 設定ファイルのパース・検証・変換モジュール

use std::path::{Path, PathBuf};

use garde::Validate;
use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{
  font::{Feature, FontConfig, FontConfigs, FontType, TextDirection, VariationAxis},
  project::{ProjectPath, ProjectSource},
};

mod pre_config;
use pre_config::{PreConfig, PreFontConfig};
mod processed_config;
mod tag;

#[doc(hidden)]
pub mod test_support;

pub use processed_config::{Config, DocumentConfig, ImageConfig, Margin, OutputConfig, PdfConfig};

/// 設定ファイル読み込みで発生するすべてのエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum ReadConfigError {
  /// 設定ファイルの読み込み失敗
  #[error("設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// 読み込みに失敗した設定ファイルのパス
    path: String,
    #[source]
    /// 元の I/O エラー
    source: std::io::Error,
  },
  /// TOML 解析失敗
  #[error("設定ファイルの TOML 解析に失敗しました")]
  #[diagnostic(code(config::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    #[source_code]
    /// エラー位置を示すためのソース全文
    src: NamedSource<String>,
    #[label("ここ")]
    /// エラー箇所のソース内スパン
    span: SourceSpan,
    #[source]
    /// 元の TOML パースエラー
    source: toml::de::Error,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("複数のバリデーションエラーが発生しました。")]
  #[diagnostic(code(config::multiple_validation_errors))]
  MultipleValidationErrors {
    #[related]
    /// 集約された個々のバリデーションエラー
    errors: Vec<ConfigValidationError>,
  },
}

/// 設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ConfigValidationError {
  /// garde が検出した設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(code(config::validation::field), help("config.toml の該当フィールドの値を確認してください。"))]
  Field {
    /// 不正な値を持つフィールドの TOML パス（例: `pdf.margin_top`）
    path: String,
    /// garde が生成したエラーメッセージ
    message: String,
  },
  /// フォントパスが見つからない
  #[error("フォントファイルが見つかりません: {path}")]
  #[diagnostic(
    code(config::validation::font_path),
    help("フォントファイルが存在し、読み取り権限があることを確認してください。")
  )]
  FontPathResolution {
    /// 対象のフォント種別
    font_type: FontType,
    /// 見つからなかったフォントファイルのパス
    path: String,
  },
  /// スタイル設定ファイルが見つからない
  #[error("スタイル設定ファイルが見つかりません: {path}")]
  #[diagnostic(
    code(config::validation::style_path),
    help("スタイル設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  StylePathResolution {
    /// 見つからなかったスタイル設定ファイルのパス
    path: String,
  },
  /// 参照設定ファイルが見つからない
  #[error("参照設定ファイルが見つかりません: {path}")]
  #[diagnostic(
    code(config::validation::references_path),
    help("参照設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  ReferencesPathResolution {
    /// 見つからなかった参照設定ファイルのパス
    path: String,
  },
  /// ソースファイルが見つからない
  #[error("ソースファイルが見つかりません: {path}")]
  #[diagnostic(
    code(config::validation::source_path),
    help("`sources` に列挙したファイルが存在し、読み取り権限があることを確認してください。")
  )]
  SourcePathResolution {
    /// 見つからなかったソースファイルのパス
    path: String,
  },
}

/// 読み取り I/O フェーズで集約する解決済みパス群。
///
/// `font_paths` は [`FontType::ALL`] の順序に対応する正規化済みフォントパスで、エラーが
/// ない場合のみ 19 要素が揃います。
struct ResolvedPaths {
  /// `FontType::ALL` の順序に対応する正規化済みフォントパス
  font_paths: Vec<PathBuf>,
  /// 正規化済みソースファイルパス
  sources: Vec<PathBuf>,
  /// 正規化済みスタイルファイルパス（未指定なら `None`）
  style_path: Option<PathBuf>,
  /// 正規化済み参照定義ファイルパス（未指定なら `None`）
  references_path: Option<PathBuf>,
}

/// フォント 1 種別ぶんの、パス解決を除いた検証済み・変換済みの値群。
struct FontValues {
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
/// `base_dir` は相対パス（`sources` / `style_path` / フォントパス等）の解決基準ディレクトリです。
/// 呼び出し元がカレントディレクトリ等を決めて渡します（本関数は `std::env::current_dir` を呼びません）。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション・出力パス構築の失敗時にエラーを返します。
// `ReadConfigError::ParseToml` が `NamedSource<String>` を保持して Result サイズが拡大するため
// allow する。`config.toml` は 1 回しか読まないので最適化対象ではない。
#[allow(clippy::result_large_err)]
pub fn read_config(source: &dyn ProjectSource, config_path: &Path, base_dir: &Path) -> Result<Config, ReadConfigError> {
  debug!(config_path = %config_path.display(), "設定ファイルの読み込みを開始します");
  let config_content = source.read_text(&ProjectPath::new(config_path)).map_err(|source| {
    return ReadConfigError::ReadFile {
      path: config_path.display().to_string(),
      source: source.into_io(),
    };
  })?;
  let pre_config = parse_config(&config_content, config_path)?;
  let config = resolve(pre_config, source, base_dir)?;

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
#[allow(clippy::result_large_err)]
fn parse_config(content: &str, source_path: &Path) -> Result<PreConfig, ReadConfigError> {
  return toml::from_str(content).map_err(|mut source| {
    let span = source.span().map_or_else(
      || return SourceSpan::new(0.into(), 0),
      |range| return SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
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

/// [`PreConfig`] からパス解決を行い [`Config`] を構築します。
///
/// 値検証と読み取り I/O の違反を集約します。出力ディレクトリの作成は行わず、絶対パスを
/// 組み立てるだけです（作成は driver 側の責務、#300）。
#[allow(clippy::result_large_err)]
fn resolve(pre: PreConfig, source: &dyn ProjectSource, base_dir: &Path) -> Result<Config, ReadConfigError> {
  let validation = validate_and_convert(&pre);
  let (resolved, path_errors) = resolve_paths(&pre, source, base_dir);

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

  let output_dir = resolve_output_dir_path(base_dir, pre.output.output_dir.as_deref());

  let PreConfig {
    document: pre_document,
    output: pre_output,
    pdf: pre_pdf_config,
    image: pre_image_config,
    ..
  } = pre;

  let font_configs =
    FontConfigs::from_all(font_values.into_iter().zip(resolved.font_paths).map(|(values, font_path)| {
      return FontConfig {
        font_path,
        font_index: values.font_index,
        variation_axes: values.variation_axes,
        script: values.script,
        language: values.language,
        ot_language_tag: values.ot_language_tag,
        direction: values.direction,
        features: values.features,
      };
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
      show_bookmarks: pre_pdf_config.show_bookmarks,
    },
    image: ImageConfig {
      max_dpi: pre_image_config.max_dpi,
      downsample: pre_image_config.downsample,
    },
    font_configs,
    sources: resolved.sources,
    style_path: resolved.style_path,
    references_path: resolved.references_path,
  });
}

/// [`PreConfig`] の純粋な値検証とタグ・書字方向の変換を一括で実行します（I/O なし）。
fn validate_and_convert(pre: &PreConfig) -> Result<Vec<FontValues>, Vec<ConfigValidationError>> {
  let mut errors: Vec<ConfigValidationError> = Vec::new();
  if let Err(report) = pre.validate() {
    errors.extend(report.iter().map(|(path, error)| {
      return ConfigValidationError::Field {
        path: path.to_string(),
        message: error.to_string(),
      };
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
#[cfg(test)]
fn validate_values(pre: &PreConfig) -> Result<(), Vec<ConfigValidationError>> {
  return validate_and_convert(pre).map(|_| ());
}

/// 読み取り I/O フェーズ: フォント・ソース・スタイル・参照のパスを `source.exists` で確認します。
///
/// `canonicalize`（実ディスクの symlink 解決込みの存在確認）は使わず、`ProjectPath` へ正規化した
/// パスの存在有無だけを見る。`MemoryProjectSource` のような実ファイルシステムに触れないテスト
/// adapter と両立させるための変更（issue #300）。
fn resolve_paths(
  pre: &PreConfig,
  source: &dyn ProjectSource,
  base_dir: &Path,
) -> (ResolvedPaths, Vec<ConfigValidationError>) {
  let mut errors: Vec<ConfigValidationError> = Vec::new();

  let style_path = resolve_optional_path(pre.style_path.as_deref(), base_dir, source, &mut errors, |path| {
    return ConfigValidationError::StylePathResolution { path };
  });
  let references_path = resolve_optional_path(pre.references_path.as_deref(), base_dir, source, &mut errors, |path| {
    return ConfigValidationError::ReferencesPathResolution { path };
  });

  let mut font_paths: Vec<PathBuf> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    let pre_font_config = pre.font_configs.get(font_type);
    let joined = join_with_base(&pre_font_config.font_path, base_dir);
    if source.exists(&ProjectPath::new(&joined)) {
      font_paths.push(joined);
    } else {
      errors.push(ConfigValidationError::FontPathResolution {
        font_type,
        path: joined.display().to_string(),
      });
    }
  }

  let sources = resolve_sources(&pre.sources, base_dir, source, &mut errors);

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

/// 相対パスを `base_dir` へ結合します（絶対パスはそのまま）。
fn join_with_base(path: &Path, base_dir: &Path) -> PathBuf {
  if path.is_absolute() {
    return path.to_path_buf();
  }
  return base_dir.join(path);
}

/// オプションのパスを `base_dir` へ結合し、`source.exists` で存在確認します。
fn resolve_optional_path(
  path: Option<&Path>,
  base_dir: &Path,
  source: &dyn ProjectSource,
  errors: &mut Vec<ConfigValidationError>,
  make_err: impl FnOnce(String) -> ConfigValidationError,
) -> Option<PathBuf> {
  let p = path?;
  let joined = join_with_base(p, base_dir);
  if source.exists(&ProjectPath::new(&joined)) {
    return Some(joined);
  }
  errors.push(make_err(joined.display().to_string()));
  return None;
}

/// 各 source パスを `base_dir` へ結合し、`source.exists` で存在確認します。
fn resolve_sources(
  sources: &[PathBuf],
  base_dir: &Path,
  project_source: &dyn ProjectSource,
  errors: &mut Vec<ConfigValidationError>,
) -> Vec<PathBuf> {
  let mut resolved = Vec::with_capacity(sources.len());
  for source_path in sources {
    if source_path.extension().and_then(|ext| return ext.to_str()) != Some("sei") {
      warn!(
        source_path = %source_path.display(),
        "ソースファイルの拡張子が `.sei` ではありません（`.sei` を推奨します）"
      );
    }
    let joined = join_with_base(source_path, base_dir);
    if project_source.exists(&ProjectPath::new(&joined)) {
      resolved.push(joined);
    } else {
      errors.push(ConfigValidationError::SourcePathResolution {
        path: joined.display().to_string(),
      });
    }
  }
  return resolved;
}

/// `PreFontConfig` のタグ・書字方向を検証・変換し、[`FontValues`] を生成します（I/O なし）。
fn parse_font_values(
  font_type: FontType,
  pre_font_config: &PreFontConfig,
) -> Result<FontValues, Vec<ConfigValidationError>> {
  let mut errors: Vec<ConfigValidationError> = Vec::new();

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

  let variation_axes = pre_font_config.variation_axes.as_deref().map(|axes| {
    return axes
      .iter()
      .filter_map(|axis| match tag::parse_opentype_tag(&axis.name) {
        Ok(name) => {
          return Some(VariationAxis {
            name,
            value: axis.value,
          });
        },
        Err(error) => {
          errors.push(field_error(font_type, "variation_axes", error));
          return None;
        },
      })
      .collect::<Vec<_>>();
  });

  let features = pre_font_config.features.as_deref().and_then(|feats| {
    let converted: Vec<Feature> = feats
      .iter()
      .filter_map(|feature| match tag::parse_opentype_tag(&feature.tag) {
        Ok(tag) => {
          return Some(Feature {
            tag,
            value: feature.value,
          });
        },
        Err(error) => {
          errors.push(field_error(font_type, "features", error));
          return None;
        },
      })
      .collect();
    return (!converted.is_empty()).then_some(converted);
  });

  let language = build_language_string(pre_font_config.language.as_deref(), pre_font_config.ot_language.as_deref());

  if !errors.is_empty() {
    return Err(errors);
  }
  return Ok(FontValues {
    font_index: pre_font_config.font_index,
    variation_axes,
    script,
    language,
    ot_language_tag,
    direction,
    features,
  });
}

/// フォント種別とフィールド名から、タグ・書字方向の不正を表す [`ConfigValidationError::Field`] を作ります。
fn field_error(font_type: FontType, field: &str, error: impl std::fmt::Display) -> ConfigValidationError {
  return ConfigValidationError::Field {
    path: format!("font_configs.{}.{field}", font_type.as_toml_key()),
    message: error.to_string(),
  };
}

/// BCP 47 言語タグと OT 言語タグから、harfrust の [`Language::from_str`] に渡す最終 BCP 47 文字列を構築します。
///
/// `ot_language` が指定されている場合は、ベースの BCP 47（未指定なら `"und"`）の末尾に
/// `-x-hbot<TAG>` 予約サブタグを連結します。
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
/// `output_dir` が相対パスまたは未指定の場合は `base_dir` を基準に解決します。
fn resolve_output_dir_path(base_dir: &Path, output_dir: Option<&Path>) -> PathBuf {
  match output_dir {
    Some(path) if path.is_absolute() => return path.to_path_buf(),
    Some(path) => return base_dir.join(path),
    None => return base_dir.to_path_buf(),
  }
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use super::{
    Config, ConfigValidationError, ReadConfigError, TextDirection, build_language_string, parse_config, read_config,
    resolve_output_dir_path, resolve_paths, validate_values,
  };
  use crate::{
    config::config_toml::test_support::{
      font_sections_with_serif_extra, make_font_sections, valid_output_section, valid_pdf_section,
    },
    font::FontType,
    project::{FilesystemProjectSource, MemoryProjectSource},
  };

  /// `parse_config` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  /// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します
  /// （旧 `crates/config/tests/common/mod.rs` の統合テスト用ヘルパ。実ファイルシステム経由の
  /// `read_config`/`FilesystemProjectSource` の振る舞いを検証するテストでのみ使う）。
  fn setup_config(build_toml: impl FnOnce(&str, &str, &str) -> String) -> (tempfile::TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().expect("一時ディレクトリを作成できるはず");
    let font_path = tempdir.path().join("dummy.ttf");
    std::fs::write(&font_path, b"").expect("ダミーフォントを書き込めるはず");
    let source_path = tempdir.path().join("source.sei");
    std::fs::write(&source_path, b"").expect("ダミーソースを書き込めるはず");
    let output_dir = tempdir.path().join("output");
    let config_path = tempdir.path().join("config.toml");
    let toml_text =
      build_toml(font_path.to_str().unwrap(), output_dir.to_str().unwrap(), source_path.to_str().unwrap());
    std::fs::write(&config_path, toml_text).expect("config.toml を書き込めるはず");
    return (tempdir, config_path);
  }

  #[test]
  fn resolve_paths_reports_missing_paths_without_touching_disk() {
    // Arrange — MemoryProjectSource は何も登録しない＝存在しないパスとして扱われる
    let toml = format!(
      "sources = [\"a.sei\"]\nstyle_path = \"style.toml\"\nreferences_path = \"references.toml\"\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      make_font_sections("fonts/dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();
    let source = MemoryProjectSource::new();

    // Act
    let (_, errors) = resolve_paths(&pre, &source, Path::new("/project"));

    // Assert — スタイル・文献・ソース・フォント全種のパス不存在が集約されるはず
    assert!(errors.iter().any(|e| matches!(e, ConfigValidationError::StylePathResolution { .. })));
    assert!(errors.iter().any(|e| matches!(e, ConfigValidationError::ReferencesPathResolution { .. })));
    assert!(errors.iter().any(|e| matches!(e, ConfigValidationError::SourcePathResolution { .. })));
    assert!(errors.iter().any(|e| matches!(e, ConfigValidationError::FontPathResolution { .. })));
  }

  #[test]
  fn resolve_paths_succeeds_when_memory_source_has_all_paths() {
    // Arrange
    let toml = format!(
      "style_path = \"style.toml\"\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      make_font_sections("fonts/dummy.ttf"),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();
    let source = MemoryProjectSource::new()
      .with_text("/project/style.toml", "")
      .with_bytes("/project/fonts/dummy.ttf", Vec::new());

    // Act
    let (resolved, errors) = resolve_paths(&pre, &source, Path::new("/project"));

    // Assert
    assert!(errors.is_empty(), "登録済みパスはエラーにならないはず: {errors:?}");
    assert_eq!(resolved.style_path, Some(PathBuf::from("/project/style.toml")));
    assert!(resolved.references_path.is_none());
  }

  #[test]
  fn read_config_does_not_create_the_output_directory() {
    // Arrange — 存在しない出力ディレクトリを指す config を MemoryProjectSource で読む。
    // output_dir は実ディスク上の tempdir 配下の絶対パスにする（絶対パスは
    // resolve_output_dir_path でそのまま使われるため MemoryProjectSource フィクスチャと
    // 矛盾しない）。旧実装ならここで fs::create_dir_all が実際にディレクトリを作ってしまうため、
    // 「存在しないパスを検証する」だけの空振りテストにならない。
    let tempdir = tempfile::tempdir().expect("一時ディレクトリを作成できるはず");
    let output_dir = tempdir.path().join("does-not-exist-yet");
    let toml = format!(
      "sources = [\"a.sei\"]\n\n{}{}{}",
      valid_output_section("test", output_dir.to_str().unwrap()),
      valid_pdf_section(),
      make_font_sections("fonts/dummy.ttf"),
    );
    let source = MemoryProjectSource::new()
      .with_text("/project/config.toml", &toml)
      .with_bytes("/project/a.sei", Vec::new())
      .with_bytes("/project/fonts/dummy.ttf", Vec::new());

    // Act
    let result = read_config(&source, Path::new("/project/config.toml"), Path::new("/project"));

    // Assert — 出力ディレクトリの作成は driver 側の責務になり、config は作らない
    result.expect("fixture は妥当な最小 config のはず");
    assert!(!output_dir.exists(), "config は出力ディレクトリを作成してはいけない");
  }

  #[test]
  fn resolve_output_dir_path_keeps_absolute_path_as_is() {
    // Arrange
    let current_dir = Path::new("/home/user/project");
    let output_dir = PathBuf::from("/var/out");

    // Act
    let resolved = resolve_output_dir_path(current_dir, Some(&output_dir));

    // Assert
    assert_eq!(resolved, PathBuf::from("/var/out"));
  }

  #[test]
  fn resolve_output_dir_path_joins_relative_path_to_current_dir() {
    // Arrange
    let current_dir = Path::new("/home/user/project");
    let output_dir = PathBuf::from("build/out");

    // Act
    let resolved = resolve_output_dir_path(current_dir, Some(&output_dir));

    // Assert
    assert_eq!(resolved, PathBuf::from("/home/user/project/build/out"));
  }

  #[test]
  fn resolve_output_dir_path_uses_current_dir_when_none() {
    // Arrange
    let current_dir = Path::new("/home/user/project");

    // Act
    let resolved = resolve_output_dir_path(current_dir, None);

    // Assert
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
    // Arrange
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
    assert!(span.offset() > 0 || !span.is_empty(), "span must point at the syntax issue, got {span:?}");
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
      ConfigValidationError::Field { path, .. } if path.contains("margin_top")
    )));
  }

  #[test]
  fn validate_values_fails_on_margin_sum_exceeding_dimension() {
    // Arrange
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
      ConfigValidationError::Field { path, message } if path == "pdf" && message.contains("vertical")
    )));
  }

  #[test]
  fn validate_values_fails_on_duplicate_font_names_with_font_type_in_path() {
    // Arrange
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
        ConfigValidationError::Field { path, message } if message.contains("重複") => return Some(path.as_str()),
        _ => return None,
      })
      .expect("expected duplicate font name error");
    assert_eq!(dup_path, "font_configs.serif_bold");
  }

  #[test]
  fn parse_config_fails_on_legacy_top_level_name() {
    // Arrange
    let toml = format!(
      "name = \"test\"\n\n[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
       margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\n{}",
      make_font_sections("dummy.ttf"),
    );

    // Act
    let result = parse_config(&toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadConfigError::ParseToml { .. })));
  }

  #[test]
  fn validate_values_fails_on_empty_sources() {
    // Arrange
    let toml = format!(
      "sources = []\n\n{}{}{}",
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
      ConfigValidationError::Field { path, .. } if path == "sources"
    )));
  }

  #[test]
  fn validate_values_fails_on_omitted_sources() {
    // Arrange
    let toml =
      format!("{}{}{}", valid_output_section("test", "out"), valid_pdf_section(), make_font_sections("dummy.ttf"));
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ConfigValidationError::Field { path, .. } if path == "sources"
    )));
  }

  #[test]
  fn validate_values_fails_on_empty_output_dir() {
    // Arrange
    let toml =
      format!("{}{}{}", valid_output_section("test", ""), valid_pdf_section(), make_font_sections("dummy.ttf"));
    let pre = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ConfigValidationError::Field { path, .. } if path == "output.output_dir"
    )));
  }

  #[test]
  fn validate_values_fails_on_out_of_range_max_dpi() {
    // Arrange
    let toml = format!(
      "sources = [\"dummy.sei\"]\n\n{}{}[image]\nmax_dpi = 9999\n\n{}",
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
      ConfigValidationError::Field { path, .. } if path == "image.max_dpi"
    )));
  }

  /// 指定の `[font_configs.serif]` 追加行で TOML を組み、`validate_values` を通した結果を返します。
  fn run_validate_with_serif_extra(extra_lines: &str) -> Result<(), Vec<ConfigValidationError>> {
    let toml = format!(
      "sources = [\"dummy.sei\"]\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      crate::config::config_toml::test_support::font_sections_with_serif_extra("dummy.ttf", extra_lines),
    );
    let pre = parse_config(&toml, dummy_source()).unwrap();
    return validate_values(&pre);
  }

  #[test]
  fn validate_values_accepts_valid_bcp47_languages() {
    // Arrange / Act / Assert
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
      ConfigValidationError::Field { path, message } if path.contains("language") && message.contains("BCP 47")
    )));
  }

  #[test]
  fn validate_values_rejects_reserved_private_use_in_language() {
    // Arrange / Act
    for forbidden in ["en-x-hbsclatn", "ja-x-hbotJAN"] {
      let extra = format!("language = \"{forbidden}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ConfigValidationError::Field { path, message }
            if path.contains("language") && (message.contains("-x-hbsc") || message.contains("-x-hbot"))
        )),
        "expected '{forbidden}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_accepts_structurally_valid_ot_script_tags() {
    // Arrange / Act / Assert
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
    // Arrange / Act / Assert
    for script in ["kan", "kanaa", "kan1", "ka一"] {
      let extra = format!("script = \"{script}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ConfigValidationError::Field { path, .. } if path.contains("script")
        )),
        "expected script='{script}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_accepts_valid_ot_language_with_script() {
    // Arrange / Act / Assert
    for ot_lang in ["JAN", "ENG", "DEU", "ZHS"] {
      let extra = format!("script = \"latn\"\not_language = \"{ot_lang}\"");
      assert!(run_validate_with_serif_extra(&extra).is_ok(), "expected ot_language='{ot_lang}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_ot_language_tag() {
    // Arrange / Act / Assert
    for ot_lang in ["JA", "JAPAN", "J!N"] {
      let extra = format!("script = \"latn\"\not_language = \"{ot_lang}\"");
      let errors = run_validate_with_serif_extra(&extra).unwrap_err();
      assert!(
        errors.iter().any(|error| matches!(
          error,
          ConfigValidationError::Field { path, .. } if path.contains("ot_language")
        )),
        "expected ot_language='{ot_lang}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_rejects_ot_language_without_script() {
    // Arrange
    let extra = "ot_language = \"JAN\"";

    // Act
    let errors = run_validate_with_serif_extra(extra).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ConfigValidationError::Field { path, message }
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
    // Arrange / Act / Assert
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
    // Arrange / Act / Assert
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
          ConfigValidationError::Field { path, .. } if path.contains("direction")
        )),
        "expected direction='{direction}' to be rejected"
      );
    }
  }

  #[test]
  fn validate_values_accepts_valid_document_language() {
    // Arrange / Act / Assert
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
      ConfigValidationError::Field { path, message } if path.contains("language") && message.contains("BCP 47")
    )));
  }

  #[test]
  fn validate_values_rejects_empty_keyword_entry() {
    // Arrange
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
      ConfigValidationError::Field { path, message } if path.contains("keywords") && message.contains("空")
    )));
  }

  // 以下は旧 `crates/config/tests/config.rs`（`read_config` の公開 API を実ファイルシステム経由で
  // 検証する統合テスト）を移設したもの。上のテスト群が `MemoryProjectSource` で内部関数を
  // 直接検証するのに対し、こちらは `FilesystemProjectSource` + tempfile で `read_config` を
  // end-to-end に検証する。

  #[test]
  fn read_config_succeeds_with_valid_config() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n[document]\ntitle = \"Test Doc\"\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    assert_eq!(config.output.name, "test_doc");
    assert_eq!(config.document.title.as_deref(), Some("Test Doc"));
    assert_eq!(config.sources.len(), 1);
    assert_eq!(config.font_configs.iter().count(), 19);
    assert!(config.pdf.show_bookmarks);
    assert_eq!(config.image.max_dpi, 300);
    assert!(config.image.downsample);
  }

  #[test]
  fn read_config_reads_image_overrides() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}[image]\nmax_dpi = 150\ndownsample = false\n\n{}",
        valid_output_section("test", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    assert_eq!(config.image.max_dpi, 150);
    assert!(!config.image.downsample);
  }

  #[test]
  fn read_config_respects_show_bookmarks_false() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n{}[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\
         margin_top = \"50pt\"\nmargin_bottom = \"50pt\"\nmargin_left = \"50pt\"\nmargin_right = \"50pt\"\n\
         show_bookmarks = false\n\n{}",
        valid_output_section("test", output_dir),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    assert!(!config.pdf.show_bookmarks);
  }

  #[test]
  fn read_config_fails_on_nonexistent_font_path() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|_font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test", output_dir),
        valid_pdf_section(),
        make_font_sections("/nonexistent/path/to/font.ttf"),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let result = read_config(&source, &config_path, &base_dir);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().all(|error| matches!(error, ConfigValidationError::FontPathResolution { .. })));
    assert_eq!(errors.len(), 19);
  }

  #[test]
  fn read_config_fails_on_nonexistent_source_path() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, _source_path| {
      return format!(
        "sources = [\"/nonexistent/source.sei\"]\n\n{}{}{}",
        valid_output_section("test", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let result = read_config(&source, &config_path, &base_dir);

    // Assert
    let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().any(|error| matches!(error, ConfigValidationError::SourcePathResolution { .. })));
  }

  #[test]
  fn read_config_uses_base_dir_when_output_dir_omitted() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, _output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n[output]\nname = \"out\"\n\n{}{}",
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();

    // Act
    let config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert — output_dir 省略時は base_dir がそのまま使われる（呼び出し元がその意味付けを担う）
    assert_eq!(config.output.output_dir, base_dir);
    assert_eq!(config.output.pdf_path(), base_dir.join("out.pdf"));
  }

  #[test]
  fn read_config_preserves_user_script_tag_case() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "script = \"Latn\"";
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      );
    });

    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.script, Some(*b"Latn"));
  }

  #[test]
  fn read_config_builds_language_string_with_ot_language_suffix() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "language = \"ja\"\nscript = \"kana\"\not_language = \"JAN\"";
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.language.as_deref(), Some("ja-x-hbotJAN"));
    assert_eq!(serif.script, Some(*b"kana"));
    assert_eq!(serif.ot_language_tag, Some(*b"JAN "));
  }

  #[test]
  fn read_config_builds_language_string_with_und_base_when_only_ot_language() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "script = \"latn\"\not_language = \"ENG\"";
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.language.as_deref(), Some("und-x-hbotENG"));
    assert_eq!(serif.ot_language_tag, Some(*b"ENG "));
  }

  #[test]
  fn read_config_preserves_user_direction() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let extra = "direction = \"right-to-left\"";
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        font_sections_with_serif_extra(font_path, extra),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    let serif = config.font_configs.get(FontType::Serif);
    assert_eq!(serif.direction, Some(TextDirection::RightToLeft));
  }

  #[test]
  fn read_config_preserves_document_language_and_keywords() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n[document]\nlanguage = \"ja\"\nkeywords = [\"組版\", \"PDF\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    assert_eq!(config.document.language.as_deref(), Some("ja"));
    assert_eq!(config.document.keywords.as_deref(), Some(&["組版".to_string(), "PDF".to_string()][..]));
  }

  #[test]
  fn read_config_keeps_document_language_and_keywords_none_when_omitted() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    assert_eq!(config.document.language, None);
    assert_eq!(config.document.keywords, None);
  }

  #[test]
  fn read_config_keeps_direction_none_when_omitted() {
    // Arrange
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      return format!(
        "sources = [\"{source_path}\"]\n\n{}{}{}",
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let config: Config = read_config(&source, &config_path, &base_dir).unwrap();

    // Assert
    for font_type in FontType::ALL {
      assert_eq!(config.font_configs.get(font_type).direction, None, "{font_type:?}");
    }
  }
}
