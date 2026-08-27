//! `config.toml`（物理・実体・メタデータ）のデータモデルと読込・検証
//!
//! [`load`] は TOML 解析・未知キー拒否・必須フィールドと値域の検証・フォント 19 種の完全性検証・
//! 各種パスの正規化までを終えた [`ProjectConfig`] を返す。`style.toml` や references の**内容は
//! 解析しない** — 検証済みのパスを返すだけで、それぞれの読込は [`crate::style`] と
//! `crate::semantics` が担う。

use std::path::{Path, PathBuf};

use garde::Validate;
use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;
use tracing::debug;

use crate::{
  failures::Failures,
  project::{
    Feature, FontConfig, FontConfigs, FontType, ProjectPath, ProjectSource, SourceReadError, TextDirection,
    VariationAxis,
  },
};

mod raw;
use raw::{RawConfig, RawFontConfig};
mod resolved;
mod tag;

#[doc(hidden)]
pub mod test_support;

pub(crate) use resolved::{DocumentConfig, ImageConfig, OutputConfig, PdfConfig, ProjectConfig};

/// 設定ファイル読み込みで発生するすべてのエラー。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ReadConfigError {
  /// 設定ファイルの読み込み失敗
  #[error("設定ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(project::config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// 読み込みに失敗した設定ファイルのパス
    path: String,
    #[source]
    /// 元の読み込みエラー（低水準 cause）
    source: SourceReadError,
  },
  /// TOML 解析失敗
  #[error("設定ファイルの TOML 解析に失敗しました")]
  #[diagnostic(code(project::config::parse_toml), help("TOML の構文を確認してください。"))]
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
  /// 値検証の違反 1 件
  ///
  /// 複数の違反は `Failures<ReadConfigError>` の別要素として並ぶ。段名だけを表す集約
  /// バリアント（旧 `MultipleValidationErrors`）は持たない — ユーザーが最初に読むのは
  /// 「どのフィールドをどう直すか」であるべきで、「複数のバリデーションエラー」ではない（#376）。
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] ConfigValidationError),
}

/// 設定値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ConfigValidationError {
  /// garde が検出した設定値の不正
  #[error("'{path}': {message}")]
  #[diagnostic(
    code(project::config::validation::field),
    help("config.toml の該当フィールドの値を確認してください。")
  )]
  Field {
    /// 不正な値を持つフィールドの TOML パス（例: `pdf.width`）
    path: String,
    /// garde が生成したエラーメッセージ
    message: String,
  },
  /// フォントパスが見つからない
  #[error("フォントファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::font_path),
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
    code(project::config::validation::style_path),
    help("スタイル設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  StylePathResolution {
    /// 見つからなかったスタイル設定ファイルのパス
    path: String,
  },
  /// 参照設定ファイルが見つからない
  #[error("参照設定ファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::references_path),
    help("参照設定ファイルが存在し、読み取り権限があることを確認してください。")
  )]
  ReferencesPathResolution {
    /// 見つからなかった参照設定ファイルのパス
    path: String,
  },
  /// ソースファイルが見つからない
  #[error("ソースファイルが見つかりません: {path}")]
  #[diagnostic(
    code(project::config::validation::source_path),
    help("`sources` に列挙したファイルが存在し、読み取り権限があることを確認してください。")
  )]
  SourcePathResolution {
    /// 見つからなかったソースファイルのパス
    path: String,
  },
}

/// config.toml の警告（読み込みは成功するが、ユーザーが直したほうがよい問題）。
///
/// エラー（[`ConfigValidationError`]）と型を分けているのは、warning が成功した
/// `Compilation` と一緒に返り `CompileFailure` には混ざらないため（#377）。
#[derive(Debug, Clone, Error, Diagnostic)]
pub(crate) enum ConfigWarning {
  /// `sources` のファイル拡張子が `.sei` ではない。
  #[error("ソースファイルの拡張子が `.sei` ではありません: {path}")]
  #[diagnostic(
    code(project::config::source_extension),
    severity(Warning),
    help("Seiran のソースファイルには拡張子 `.sei` を使ってください。")
  )]
  SourceExtension {
    /// config.toml に書かれたままのソースパス
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
/// 成功時は検証済みの [`ProjectConfig`] と、読み込みを止めない警告 [`ConfigWarning`] の列を
/// `sources` の宣言順で返します。
///
/// # Errors
///
/// ファイル読み込み・TOML 解析・バリデーション・出力パス構築の失敗時にエラーを返します。
pub(crate) fn load(
  source: &dyn ProjectSource,
  config_path: &Path,
  base_dir: &Path,
) -> Result<(ProjectConfig, Vec<ConfigWarning>), Failures<ReadConfigError>> {
  debug!(config_path = %config_path.display(), "設定ファイルの読み込みを開始します");
  let config_content = source.read_text(&ProjectPath::new(config_path)).map_err(|source| {
    return ReadConfigError::ReadFile {
      path: config_path.display().to_string(),
      source,
    };
  })?;
  let raw_config = parse_config(&config_content, config_path)?;
  let (config, warnings) = resolve(raw_config, source, base_dir)?;

  debug!(
    config_path = %config_path.display(),
    output_name = %config.output.name,
    output_path = %config.output.pdf_path().display(),
    warning_count = warnings.len(),
    "設定ファイルの読み込みが完了しました"
  );
  return Ok((config, warnings));
}

/// TOML 文字列を [`RawConfig`] にパースします（I/O なし）。
///
/// `source_path` はエラー報告に使う表示用パスで、ファイルシステムへのアクセスには使われません。
/// 値検証は行いません。検証・変換は [`validate_and_convert`]（[`resolve`] 経由）で実行します。
fn parse_config(content: &str, source_path: &Path) -> Result<RawConfig, Failures<ReadConfigError>> {
  return toml::from_str(content).map_err(|mut source| {
    let span = source.span().map_or_else(
      || return SourceSpan::new(0.into(), 0),
      |range| return SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    // toml::de::Error::Display は input が設定されていると line/column の自前スニペットを描画する。
    // miette の #[label] と二重に位置情報が出るため、ここで input をクリアして抑止する。
    source.set_input(None);
    return Failures::single(ReadConfigError::ParseToml {
      src: NamedSource::new(source_path.display().to_string(), content.to_string()),
      span,
      source,
    });
  });
}

/// [`RawConfig`] からパス解決を行い [`ProjectConfig`] を構築します。
///
/// 値検証と読み取り I/O の違反を集約します。出力ディレクトリの作成は行わず、絶対パスを
/// 組み立てるだけです（作成は driver 側の責務、#300）。
fn resolve(
  raw: RawConfig,
  source: &dyn ProjectSource,
  base_dir: &Path,
) -> Result<(ProjectConfig, Vec<ConfigWarning>), Failures<ReadConfigError>> {
  let validation = validate_and_convert(&raw);
  let (resolved, path_errors, warnings) = resolve_paths(&raw, source, base_dir);

  let font_values = match validation {
    Ok(font_values) if path_errors.is_empty() => font_values,
    result => {
      let mut errors = match result {
        Ok(_) => Vec::new(),
        Err(value_errors) => value_errors,
      };
      errors.extend(path_errors);
      let Some(failures) = Failures::from_vec(errors.into_iter().map(ReadConfigError::from).collect()) else {
        unreachable!("この分岐は検証エラーかパスエラーが 1 件以上あるときにだけ入る")
      };
      return Err(failures);
    },
  };

  let output_dir = resolve_output_dir_path(base_dir, raw.output.output_dir.as_deref());

  let RawConfig {
    document: raw_document,
    output: raw_output,
    pdf: raw_pdf_config,
    image: raw_image_config,
    ..
  } = raw;

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

  let config = ProjectConfig {
    document: DocumentConfig {
      title: raw_document.title,
      author: raw_document.author,
      date: raw_document.date,
      subject: raw_document.subject,
      language: raw_document.language,
      keywords: raw_document.keywords,
    },
    output: OutputConfig {
      name: raw_output.name,
      output_dir,
    },
    pdf: PdfConfig {
      height: raw_pdf_config.height,
      width: raw_pdf_config.width,
      show_bookmarks: raw_pdf_config.show_bookmarks,
    },
    image: ImageConfig {
      max_dpi: raw_image_config.max_dpi,
      downsample: raw_image_config.downsample,
    },
    font_configs,
    sources: resolved.sources,
    style_path: resolved.style_path,
    references_path: resolved.references_path,
  };
  return Ok((config, warnings));
}

/// [`RawConfig`] の純粋な値検証とタグ・書字方向の変換を一括で実行します（I/O なし）。
fn validate_and_convert(raw: &RawConfig) -> Result<Vec<FontValues>, Vec<ConfigValidationError>> {
  let mut errors: Vec<ConfigValidationError> = Vec::new();
  if let Err(report) = raw.validate() {
    errors.extend(report.iter().map(|(path, error)| {
      return ConfigValidationError::Field {
        path: path.to_string(),
        message: error.to_string(),
      };
    }));
  }
  raw::validate_unique_font_names(&raw.font_configs, &mut errors);
  raw::validate_font_language_constraints(&raw.font_configs, &mut errors);

  let mut font_values: Vec<FontValues> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    match parse_font_values(font_type, &raw.font_configs[font_type]) {
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
fn validate_values(raw: &RawConfig) -> Result<(), Vec<ConfigValidationError>> {
  return validate_and_convert(raw).map(|_| ());
}

/// 読み取り I/O フェーズ: フォント・ソース・スタイル・参照のパスを `source.exists` で確認します。
///
/// `canonicalize`（実ディスクの symlink 解決込みの存在確認）は使わず、`ProjectPath` へ正規化した
/// パスの存在有無だけを見る。`MemoryProjectSource` のような実ファイルシステムに触れないテスト
/// adapter と両立させるための変更（issue #300）。
fn resolve_paths(
  raw: &RawConfig,
  source: &dyn ProjectSource,
  base_dir: &Path,
) -> (ResolvedPaths, Vec<ConfigValidationError>, Vec<ConfigWarning>) {
  let mut errors: Vec<ConfigValidationError> = Vec::new();
  let mut warnings: Vec<ConfigWarning> = Vec::new();

  let style_path = resolve_optional_path(raw.style_path.as_deref(), base_dir, source, &mut errors, |path| {
    return ConfigValidationError::StylePathResolution { path };
  });
  let references_path = resolve_optional_path(raw.references_path.as_deref(), base_dir, source, &mut errors, |path| {
    return ConfigValidationError::ReferencesPathResolution { path };
  });

  let mut font_paths: Vec<PathBuf> = Vec::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    let raw_font_config = &raw.font_configs[font_type];
    let joined = join_with_base(&raw_font_config.font_path, base_dir);
    if source.exists(&ProjectPath::new(&joined)) {
      font_paths.push(joined);
    } else {
      errors.push(ConfigValidationError::FontPathResolution {
        font_type,
        path: joined.display().to_string(),
      });
    }
  }

  let sources = resolve_sources(&raw.sources, base_dir, source, &mut errors, &mut warnings);

  return (
    ResolvedPaths {
      font_paths,
      sources,
      style_path,
      references_path,
    },
    errors,
    warnings,
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
///
/// 拡張子が `.sei` でないものは読み込みを止めないので、`errors` ではなく `warnings` へ
/// 宣言順に積みます。
fn resolve_sources(
  sources: &[PathBuf],
  base_dir: &Path,
  project_source: &dyn ProjectSource,
  errors: &mut Vec<ConfigValidationError>,
  warnings: &mut Vec<ConfigWarning>,
) -> Vec<PathBuf> {
  let mut resolved = Vec::with_capacity(sources.len());
  for source_path in sources {
    if source_path.extension().and_then(|ext| return ext.to_str()) != Some("sei") {
      warnings.push(ConfigWarning::SourceExtension {
        path: source_path.display().to_string(),
      });
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

/// `RawFontConfig` のタグ・書字方向を検証・変換し、[`FontValues`] を生成します（I/O なし）。
fn parse_font_values(
  font_type: FontType,
  raw_font_config: &RawFontConfig,
) -> Result<FontValues, Vec<ConfigValidationError>> {
  let mut errors: Vec<ConfigValidationError> = Vec::new();

  let script = match raw_font_config.script.as_deref() {
    None => None,
    Some(value) => match tag::parse_script_tag(value) {
      Ok(bytes) => Some(bytes),
      Err(error) => {
        errors.push(field_error(font_type, "script", error));
        None
      },
    },
  };

  let ot_language_tag = match raw_font_config.ot_language.as_deref() {
    None => None,
    Some(value) => match tag::parse_ot_language_tag(value) {
      Ok(bytes) => Some(bytes),
      Err(error) => {
        errors.push(field_error(font_type, "ot_language", error));
        None
      },
    },
  };

  let direction = match raw_font_config.direction.as_deref() {
    None => None,
    Some(value) => match value.parse::<TextDirection>() {
      Ok(direction) => Some(direction),
      Err(error) => {
        errors.push(field_error(font_type, "direction", error));
        None
      },
    },
  };

  let variation_axes = raw_font_config.variation_axes.as_deref().map(|axes| {
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

  let features = raw_font_config.features.as_deref().and_then(|feats| {
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

  let language = build_language_string(raw_font_config.language.as_deref(), raw_font_config.ot_language.as_deref());

  if !errors.is_empty() {
    return Err(errors);
  }
  return Ok(FontValues {
    font_index: raw_font_config.font_index,
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
    ConfigValidationError, ConfigWarning, ProjectConfig, ReadConfigError, TextDirection, build_language_string, load,
    parse_config, resolve_output_dir_path, resolve_paths, validate_values,
  };
  use crate::project::{
    FilesystemProjectSource, FontType, MemoryProjectSource, SourceReadError,
    config::test_support::{
      font_sections_with_serif_extra, make_font_sections, valid_output_section, valid_pdf_section,
    },
  };

  /// `parse_config` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  /// 一時ディレクトリにダミーのフォントファイル・ソースファイル・`config.toml` を作成します
  /// （旧 `crates/config/tests/common/mod.rs` の統合テスト用ヘルパ。実ファイルシステム経由の
  /// `load`/`FilesystemProjectSource` の振る舞いを検証するテストでのみ使う）。
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
    let raw = parse_config(&toml, dummy_source()).unwrap();
    let source = MemoryProjectSource::new();

    // Act
    let (_, errors, _) = resolve_paths(&raw, &source, Path::new("/project"));

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
    let raw = parse_config(&toml, dummy_source()).unwrap();
    let source = MemoryProjectSource::new()
      .with_text("/project/style.toml", "")
      .with_bytes("/project/fonts/dummy.ttf", Vec::new());

    // Act
    let (resolved, errors, _) = resolve_paths(&raw, &source, Path::new("/project"));

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
    let result = load(&source, Path::new("/project/config.toml"), Path::new("/project"));

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
    let result = parse_config("name = \nthis is not valid toml", dummy_source());

    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadConfigError::ParseToml { .. })
    ));
  }

  #[test]
  fn parse_toml_error_records_span_and_suppresses_inner_display_input() {
    // Arrange
    let invalid_toml = "name = bad\n";

    // Act
    let failures = parse_config(invalid_toml, dummy_source()).unwrap_err();

    // Assert
    let ReadConfigError::ParseToml {
      src: _,
      span,
      source,
    } = failures.into_iter().next().expect("非空集合なので 1 件目があるはず")
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
  fn parse_config_reads_pdf_section_without_margins() {
    // Arrange — `[pdf]` は用紙寸法としおり出力だけを持つ（余白は style.toml の `[page]`、#389）
    let toml =
      format!("{}{}{}", valid_output_section("test", "out"), valid_pdf_section(), make_font_sections("dummy.ttf"));

    // Act
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Assert
    assert!((raw.pdf.height.to_pt() - 842.0).abs() < f32::EPSILON);
    assert!((raw.pdf.width.to_pt() - 595.0).abs() < f32::EPSILON);
    assert!(raw.pdf.show_bookmarks);
  }

  #[test]
  fn parse_config_fails_on_legacy_pdf_margin_keys() {
    // Arrange — 旧 `pdf.margin_*` を静かに無視すると既定余白へ切り替わってレイアウトが黙って
    // 変わるため、TOML 解析時に未知キーとして拒否する（#389 の意図的な破壊的変更）
    let toml = format!(
      "{}[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\nmargin_top = \"50pt\"\n\n{}",
      valid_output_section("test", "out"),
      make_font_sections("dummy.ttf"),
    );

    // Act
    let failures = parse_config(&toml, dummy_source()).unwrap_err();

    // Assert — 値検証（Validation）ではなく TOML 解析（ParseToml）の段で落ちる
    let first = failures.into_iter().next().expect("非空集合なので 1 件目があるはず");
    assert!(
      matches!(first, ReadConfigError::ParseToml { .. }),
      "旧 margin キーは deny_unknown_fields で ParseToml になるはず: {first:?}"
    );
  }

  #[test]
  fn validate_values_fails_on_duplicate_font_names_with_font_type_in_path() {
    // Arrange
    let sections = make_font_sections("dummy.ttf").replace(
      "[font_configs.serif_bold]\nfont_name = \"font_serif_bold\"",
      "[font_configs.serif_bold]\nfont_name = \"font_serif\"",
    );
    let toml = format!("{}{}{sections}", valid_output_section("test", "out"), valid_pdf_section());
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

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
      "name = \"test\"\n\n[pdf]\nheight = \"842pt\"\nwidth = \"595pt\"\n\n{}",
      make_font_sections("dummy.ttf"),
    );

    // Act
    let result = parse_config(&toml, dummy_source());

    // Assert
    assert!(matches!(
      result.as_ref().map_err(|failures| return failures.first()),
      Err(ReadConfigError::ParseToml { .. })
    ));
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
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

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
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

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
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

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
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ConfigValidationError::Field { path, .. } if path == "image.max_dpi"
    )));
  }

  /// 指定の `[font_configs.serif]` 追加行で TOML を組み、`validate_values` を通した結果を返します。
  #[expect(
    clippy::unwrap_in_result,
    reason = "呼び出し側が `extra_lines` に渡すのは `[font_configs.serif]` へ足せる妥当な行だけなので、\
              組み上がる TOML のパースは成功する。`Result` が広告しているのは後段の値検証の失敗だけ"
  )]
  fn run_validate_with_serif_extra(extra_lines: &str) -> Result<(), Vec<ConfigValidationError>> {
    let toml = format!(
      "sources = [\"dummy.sei\"]\n\n{}{}{}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
      font_sections_with_serif_extra("dummy.ttf", extra_lines),
    );
    let raw = parse_config(&toml, dummy_source()).unwrap();
    return validate_values(&raw);
  }

  #[test]
  fn validate_values_accepts_valid_bcp47_languages() {
    for lang in ["ja", "en-US", "zh-Hant", "zh-Hans-CN", "und"] {
      let extra = format!("language = \"{lang}\"");
      assert!(run_validate_with_serif_extra(&extra).is_ok(), "expected '{lang}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_bcp47_language() {
    let errors = run_validate_with_serif_extra("language = \"!!\"").unwrap_err();

    assert!(errors.iter().any(|error| matches!(
      error,
      ConfigValidationError::Field { path, message } if path.contains("language") && message.contains("BCP 47")
    )));
  }

  #[test]
  fn validate_values_rejects_reserved_private_use_in_language() {
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
    for ot_lang in ["JAN", "ENG", "DEU", "ZHS"] {
      let extra = format!("script = \"latn\"\not_language = \"{ot_lang}\"");
      assert!(run_validate_with_serif_extra(&extra).is_ok(), "expected ot_language='{ot_lang}' to be accepted");
    }
  }

  #[test]
  fn validate_values_rejects_invalid_ot_language_tag() {
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
    assert_eq!(build_language_string(None, None), None);
    assert_eq!(build_language_string(Some("ja"), None), Some("ja".to_string()));
    assert_eq!(build_language_string(None, Some("JAN")), Some("und-x-hbotJAN".to_string()));
    assert_eq!(build_language_string(Some("en-US"), Some("ENG")), Some("en-US-x-hbotENG".to_string()));
  }

  #[test]
  fn validate_values_accepts_all_valid_directions() {
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
    for lang in ["ja", "en-US", "zh-Hant", "und"] {
      let toml = format!(
        "sources = [\"dummy.sei\"]\n\n[document]\nlanguage = \"{lang}\"\n\n{}{}{}",
        valid_output_section("test", "out"),
        valid_pdf_section(),
        make_font_sections("dummy.ttf"),
      );
      let raw = parse_config(&toml, dummy_source()).unwrap();
      assert!(validate_values(&raw).is_ok(), "expected document.language='{lang}' to be accepted");
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
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

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
    let raw = parse_config(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&raw).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ConfigValidationError::Field { path, message } if path.contains("keywords") && message.contains("空")
    )));
  }

  // 以下は旧 `crates/config/tests/config.rs`（`load` の公開 API を実ファイルシステム経由で
  // 検証する統合テスト）を移設したもの。上のテスト群が `MemoryProjectSource` で内部関数を
  // 直接検証するのに対し、こちらは `FilesystemProjectSource` + tempfile で `load` を
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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

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
         show_bookmarks = false\n\n{}",
        valid_output_section("test", output_dir),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

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
    let result = load(&source, &config_path, &base_dir);

    // Assert
    let Err(failures) = result else {
      panic!("19 件のフォントパスエラーを期待");
    };
    let errors: Vec<&ReadConfigError> = failures.iter().collect();
    assert!(
      errors
        .iter()
        .all(|error| matches!(error, ReadConfigError::Validation(ConfigValidationError::FontPathResolution { .. })))
    );
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
    let result = load(&source, &config_path, &base_dir);

    // Assert
    let Err(failures) = result else {
      panic!("ソースパスエラーを期待");
    };
    assert!(
      failures
        .iter()
        .any(|error| matches!(error, ReadConfigError::Validation(ConfigValidationError::SourcePathResolution { .. })))
    );
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
    let (config, _) = load(&source, &config_path, &base_dir).unwrap();

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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

    let serif = &config.font_configs[FontType::Serif];
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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

    // Assert
    let serif = &config.font_configs[FontType::Serif];
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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

    // Assert
    let serif = &config.font_configs[FontType::Serif];
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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

    // Assert
    let serif = &config.font_configs[FontType::Serif];
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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

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
    let (config, _): (ProjectConfig, _) = load(&source, &config_path, &base_dir).unwrap();

    // Assert
    for font_type in FontType::ALL {
      assert_eq!(config.font_configs[font_type].direction, None, "{font_type:?}");
    }
  }

  #[test]
  fn load_keeps_the_io_error_kind_as_cause_when_the_file_is_missing() {
    // Arrange — 実ファイルシステム経由で存在しない config.toml を指す
    let tempdir = tempfile::tempdir().expect("一時ディレクトリを作成できるはず");
    let config_path = tempdir.path().join("does-not-exist.toml");
    let source = FilesystemProjectSource::new();

    // Act
    let failures = load(&source, &config_path, tempdir.path()).expect_err("読み込みは失敗するはず");

    // Assert — 役割つきの leaf 診断の下に、元の I/O エラー kind が cause として残る
    let ReadConfigError::ReadFile { source, .. } = failures.first() else {
      panic!("ReadFile を期待");
    };
    let SourceReadError::Io(io_error) = source else {
      panic!("filesystem adapter は Io を返すはず: {source:?}");
    };
    assert_eq!(io_error.kind(), std::io::ErrorKind::NotFound, "not found を変換後も識別できるはず");
  }

  #[test]
  fn load_keeps_invalid_utf8_as_cause() {
    // Arrange — UTF-8 として読めないバイト列を config.toml として登録する
    let source = MemoryProjectSource::new().with_bytes("/project/config.toml", vec![0xff, 0xfe]);

    // Act
    let failures =
      load(&source, Path::new("/project/config.toml"), Path::new("/project")).expect_err("読み込みは失敗するはず");

    // Assert
    let ReadConfigError::ReadFile { source, .. } = failures.first() else {
      panic!("ReadFile を期待");
    };
    assert!(matches!(source, SourceReadError::InvalidUtf8(_)), "UTF-8 エラーを識別できるはず: {source:?}");
  }

  #[test]
  fn load_warns_once_per_non_sei_source_in_declaration_order() {
    // Arrange — `.sei` でない拡張子を 2 つ、`.sei` を 1 つ宣言する
    let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
      let dir = Path::new(source_path).parent().expect("ソースは親ディレクトリを持つはず");
      let txt = dir.join("b.txt");
      let md = dir.join("a.md");
      std::fs::write(&txt, "").expect("ダミーソースを書き込めるはず");
      std::fs::write(&md, "").expect("ダミーソースを書き込めるはず");
      return format!(
        "sources = [\"{}\", \"{source_path}\", \"{}\"]\n\n{}{}{}",
        txt.display(),
        md.display(),
        valid_output_section("test_doc", output_dir),
        valid_pdf_section(),
        make_font_sections(font_path),
      );
    });

    // Act
    let source = FilesystemProjectSource::new();
    let base_dir = config_path.parent().expect("fixture パスは親ディレクトリを持つはず").to_path_buf();
    let (_, warnings) = load(&source, &config_path, &base_dir).unwrap();

    // Assert — 宣言順に 2 件（`.sei` の 1 件は警告にならない）
    let paths: Vec<&str> = warnings
      .iter()
      .map(|warning| {
        let ConfigWarning::SourceExtension { path } = warning;
        return path.as_str();
      })
      .collect();
    assert_eq!(paths.len(), 2, "`.sei` 以外の 2 件だけが警告になるはず: {paths:?}");
    assert!(paths[0].ends_with("b.txt"), "宣言順に並ぶはず: {paths:?}");
    assert!(paths[1].ends_with("a.md"), "宣言順に並ぶはず: {paths:?}");
  }
}
