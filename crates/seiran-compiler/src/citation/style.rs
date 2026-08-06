//! CSL スタイル（`.csl`）とロケール XML の読込・解析。
//!
//! `.csl` ファイルと CSL ロケール（`xml:lang` 付き locale XML）を [`crate::project::ProjectSource`]
//! 経由で読み、解析済みの [`CompiledCitationStyle`] にまとめる。表示の生成（`BibliographyDriver` の
//! 駆動）は行わない（`crate::citation::render` の責務）。

use hayagriva::{
  archive,
  citationberg::{self, IndependentStyle, Locale, LocaleCode, LocaleFile},
};
use miette::Diagnostic;
use thiserror::Error;

use crate::{
  config::Style,
  project::{ProjectPath, ProjectSource},
};

/// CSL スタイル・ロケールの読込・解析エラー。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum CitationStyleError {
  /// 引用（`\cite`）があるのに CSL スタイルが設定されていない場合。
  #[error("引用がありますが CSL スタイルが設定されていません。")]
  #[diagnostic(
    code(citation::style::missing_csl_path),
    help("style.toml の [reference].csl_path に CSL スタイル（.csl）ファイルのパスを設定してください。")
  )]
  MissingCslPath,

  /// CSL スタイル（`.csl`）ファイルの読み込みに失敗した場合。
  #[error("CSL スタイルファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(citation::style::read_style_file),
    help("style.toml の [reference].csl_path が指す .csl ファイルのパスと読み取り権限を確認してください。")
  )]
  ReadStyleFile {
    /// スタイルファイルのパス
    path: String,
    /// 元の読み込みエラー
    #[source]
    source: std::io::Error,
  },

  /// CSL スタイル（`.csl`）の解析に失敗した場合。
  #[error("CSL スタイルファイルの解析に失敗しました: {path}")]
  #[diagnostic(
    code(citation::style::parse_style),
    help(".csl が有効な独立 CSL スタイル（independent style）であることを確認してください。")
  )]
  ParseStyle {
    /// スタイルファイルのパス
    path: String,
    /// 元の citationberg パースエラー
    #[source]
    source: citationberg::XmlDeError,
  },

  /// CSL ロケール（`.xml`）ファイルの読み込みに失敗した場合。
  #[error("CSL ロケールファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(citation::style::read_locale_file),
    help("style.toml の [reference].locale_path が指す CSL ロケール XML のパスと読み取り権限を確認してください。")
  )]
  ReadLocaleFile {
    /// ロケールファイルのパス
    path: String,
    /// 元の読み込みエラー
    #[source]
    source: std::io::Error,
  },

  /// CSL ロケール（`.xml`）の解析に失敗した場合。
  #[error("CSL ロケールファイルの解析に失敗しました: {path}")]
  #[diagnostic(
    code(citation::style::parse_locale),
    help("ファイルが有効な CSL ロケール（locales-xx-YY.xml 形式）であることを確認してください。")
  )]
  ParseLocale {
    /// ロケールファイルのパス
    path: String,
    /// 元の citationberg パースエラー
    #[source]
    source: citationberg::XmlDeError,
  },
}

/// 解析済みの CSL スタイル一式（独立スタイル本体 + ロケールプール + 出力言語 override）。
///
/// `IndependentStyle::from_xml` によるパースと `load_locales` によるロケール解決を済ませた後の
/// 値で、これ以降 I/O を伴わずに `citation::render` へ渡せる。
#[derive(Debug)]
pub(crate) struct CompiledCitationStyle {
  /// CSL 独立スタイル本体
  style: IndependentStyle,
  /// 引用整形に用いるロケールプール（カスタムロケールを内蔵ロケールより前に置く）
  locales: Vec<Locale>,
  /// 出力言語（active locale）の override
  locale_override: Option<LocaleCode>,
}

impl CompiledCitationStyle {
  /// スタイル本体・ロケールプール・出力言語 override を分解して返す。
  pub(crate) fn parts(&self) -> (&IndependentStyle, &[Locale], Option<LocaleCode>) {
    return (&self.style, &self.locales, self.locale_override.clone());
  }
}

/// `style.reference.csl_path` が指す CSL スタイルファイルを読み、ロケール解決まで済ませた
/// [`CompiledCitationStyle`] を返す。
///
/// # Errors
///
/// `style.reference.csl_path` が未設定の場合、または CSL スタイル / ロケールファイルの読み込み・
/// 解析に失敗した場合に [`CitationStyleError`] を返す。
pub(crate) fn load_citation_style(
  source: &dyn ProjectSource,
  style: &Style,
) -> Result<CompiledCitationStyle, CitationStyleError> {
  let csl_path = style.reference.csl_path.as_ref().ok_or(CitationStyleError::MissingCslPath)?;
  let csl_path_str = csl_path.display().to_string();
  let style_xml = source.read_text(&ProjectPath::new(csl_path)).map_err(|source| {
    return CitationStyleError::ReadStyleFile {
      path: csl_path_str.clone(),
      source: source.into_io(),
    };
  })?;
  let csl_style = IndependentStyle::from_xml(&style_xml).map_err(|source| {
    return CitationStyleError::ParseStyle {
      path: csl_path_str,
      source,
    };
  })?;
  let (locales, locale_override) = load_locales(style, csl_style.default_locale.as_ref(), source)?;

  return Ok(CompiledCitationStyle {
    style: csl_style,
    locales,
    locale_override,
  });
}

/// 引用整形に用いるロケールプールと、出力言語（active locale）の override を組み立てる。
///
/// カスタムロケールを内蔵ロケールより前に置く。active locale は `style.reference.locale`、
/// カスタムファイルの `xml:lang`、`csl_default_locale`、en-US の順に解決する。
///
/// # Errors
///
/// ロケールファイルの読み込み・解析に失敗した場合に [`CitationStyleError`] を返す。
fn load_locales(
  style: &Style,
  csl_default_locale: Option<&LocaleCode>,
  source: &dyn ProjectSource,
) -> Result<(Vec<Locale>, Option<LocaleCode>), CitationStyleError> {
  let (custom, file_lang): (Option<Locale>, Option<LocaleCode>) = if let Some(path) = &style.reference.locale_path {
    let path_str = path.display().to_string();
    let xml = source.read_text(&ProjectPath::new(path)).map_err(|source| {
      return CitationStyleError::ReadLocaleFile {
        path: path_str.clone(),
        source: source.into_io(),
      };
    })?;
    let locale_file = LocaleFile::from_xml(&xml).map_err(|source| {
      return CitationStyleError::ParseLocale {
        path: path_str,
        source,
      };
    })?;
    let file_lang = locale_file.lang.clone();
    (Some(locale_file.into()), Some(file_lang))
  } else {
    (None, None)
  };

  let locale_override = style.reference.locale.as_ref().map(|code| return LocaleCode(code.clone())).or(file_lang);

  let active = locale_override
    .clone()
    .or_else(|| return csl_default_locale.cloned())
    .unwrap_or_else(LocaleCode::en_us);
  let mut wanted: Vec<LocaleCode> = Vec::with_capacity(3);
  for code in [
    Some(active.clone()),
    Some(LocaleCode::en_us()),
    active.fallback(),
  ]
  .into_iter()
  .flatten()
  {
    if !wanted.contains(&code) {
      wanted.push(code);
    }
  }

  let mut locales = Vec::with_capacity(wanted.len() + usize::from(custom.is_some()));
  locales.extend(custom);
  load_builtin_locales(&wanted, &mut locales);

  return Ok((locales, locale_override));
}

/// `archive::LOCALES` の CBOR から `lang`（`@xml:lang`）だけを安価に読み出すための部分デコード対象。
#[derive(serde::Deserialize)]
struct LocaleLang {
  /// ロケールの言語コード（`@xml:lang`）。
  #[serde(rename = "@xml:lang")]
  lang: Option<LocaleCode>,
}

/// `wanted` に挙げたコードに一致する内蔵ロケールだけを `archive::LOCALES`（CBOR バイト列）から
/// 復元して `out` に追加する。すべて見つかった時点で走査を打ち切る。
fn load_builtin_locales(wanted: &[LocaleCode], out: &mut Vec<Locale>) {
  let mut remaining = wanted.len();
  for bytes in archive::LOCALES {
    if remaining == 0 {
      break;
    }
    let Ok(peek) = ciborium::de::from_reader::<LocaleLang, _>(*bytes) else {
      continue;
    };
    if !peek.lang.is_some_and(|lang| return wanted.contains(&lang)) {
      continue;
    }
    if let Ok(locale) = ciborium::de::from_reader::<Locale, _>(*bytes) {
      out.push(locale);
      remaining -= 1;
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::{
    io::Write,
    path::{Path, PathBuf},
  };

  use hayagriva::citationberg::{Locale, LocaleCode, LocaleFile};

  use super::{CitationStyleError, load_citation_style, load_locales};
  use crate::{
    citation::test_fixtures::ieee_csl_path,
    config::Style,
    project::{FilesystemProjectSource, MemoryProjectSource},
  };

  /// テスト用カスタムロケールへの絶対パスを返す。
  fn custom_locale_path() -> PathBuf {
    return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/custom-en-US.xml");
  }

  /// CSL スタイルとカスタムロケールを設定した `Style` を作る。
  fn style_with_locale_path(path: PathBuf) -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(ieee_csl_path());
    style.reference.locale_path = Some(path);
    return style;
  }

  /// ロケールの言語コードを返す。
  fn lang_of(locale: &Locale) -> Option<&str> { return locale.lang.as_ref().map(|code| return code.0.as_str()); }

  #[test]
  fn load_locales_without_custom_loads_only_active() {
    // Arrange
    let style = Style::default();
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) = load_locales(&style, None, &source).expect("内蔵 en-US のみで成功するはず");

    // Assert
    assert_eq!(locales.len(), 1, "active=en-US なら en-US 1 件だけ: {locales:?}");
    assert_eq!(lang_of(&locales[0]), Some("en-US"));
    assert!(locale_override.is_none(), "override 指定が無ければ None");
  }

  #[test]
  fn load_locales_overlays_custom_before_builtin() {
    // Arrange
    let style = style_with_locale_path(custom_locale_path());
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) =
      load_locales(&style, None, &source).expect("カスタムロケールの読み込みは成功するはず");

    // Assert
    let xml = std::fs::read_to_string(custom_locale_path()).expect("フィクスチャを読めるはず");
    let expected: Locale = LocaleFile::from_xml(&xml).expect("フィクスチャは有効な CSL ロケールのはず").into();
    assert_eq!(locales[0], expected, "先頭はカスタムロケール（同一言語コードはカスタム優先）");
    assert!(
      locales[1..].iter().any(|locale| return lang_of(locale) == Some("en-US")),
      "内蔵 en-US フォールバックが続くはず: {locales:?}"
    );
    assert_eq!(locale_override.expect("ファイル言語が override 既定になる").0.as_str(), "en-US");
  }

  #[test]
  fn load_locales_explicit_locale_overrides_file_lang() {
    // Arrange
    let mut style = style_with_locale_path(custom_locale_path());
    style.reference.locale = Some("ja-JP".to_string());
    let source = FilesystemProjectSource::new();

    // Act
    let (_locales, locale_override) = load_locales(&style, None, &source).expect("読み込みは成功するはず");

    // Assert
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_explicit_locale_loads_active_and_fallback() {
    // Arrange
    let mut style = Style::default();
    style.reference.locale = Some("ja-JP".to_string());
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) = load_locales(&style, None, &source).expect("成功するはず");

    // Assert
    let langs: Vec<&str> = locales.iter().filter_map(lang_of).collect();
    assert!(langs.contains(&"ja-JP"), "明示 locale ja-JP を読むはず: {langs:?}");
    assert!(langs.contains(&"en-US"), "en-US フォールバックも読むはず: {langs:?}");
    assert!(langs.len() < 10, "必要な数件のみで全ロケールは読まないはず: {langs:?}");
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_uses_csl_default_when_no_override() {
    // Arrange
    let style = Style::default();
    let csl_default = LocaleCode("de-DE".to_string());
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) = load_locales(&style, Some(&csl_default), &source).expect("成功するはず");

    // Assert
    let langs: Vec<&str> = locales.iter().filter_map(lang_of).collect();
    assert!(langs.contains(&"de-DE"), ".csl default の de-DE を読むはず: {langs:?}");
    assert!(langs.contains(&"en-US"), "en-US フォールバックも読むはず: {langs:?}");
    assert!(locale_override.is_none(), "明示 override が無ければ None のまま");
  }

  #[test]
  fn load_locales_reports_missing_file() {
    // Arrange
    let style = style_with_locale_path(PathBuf::from("/nonexistent/locales-en-US.xml"));
    let source = FilesystemProjectSource::new();

    // Act
    let error = load_locales(&style, None, &source).expect_err("読み込み失敗するはず");

    // Assert
    assert!(matches!(error, CitationStyleError::ReadLocaleFile { .. }), "got: {error:?}");
  }

  #[test]
  fn load_locales_reports_malformed_file() {
    // Arrange
    let mut file = tempfile::Builder::new().suffix(".xml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(b"this is not a CSL locale").expect("一時ファイルへ書き込めるはず");
    let style = style_with_locale_path(file.path().to_path_buf());
    let source = FilesystemProjectSource::new();

    // Act
    let error = load_locales(&style, None, &source).expect_err("解析失敗するはず");

    // Assert
    assert!(matches!(error, CitationStyleError::ParseLocale { .. }), "got: {error:?}");
  }

  #[test]
  fn load_citation_style_reads_csl_through_project_source() {
    // Arrange
    let csl_xml = std::fs::read_to_string(ieee_csl_path()).expect("fixture CSL を読めるはず");
    let source = MemoryProjectSource::new().with_text("/project/ieee.csl", csl_xml);
    let mut style = Style::default();
    style.reference.csl_path = Some(PathBuf::from("/project/ieee.csl"));

    // Act
    let compiled = load_citation_style(&source, &style);

    // Assert
    assert!(compiled.is_ok(), "seam 経由で CSL を読めるはず: {compiled:?}");
    assert_eq!(source.read_count("/project/ieee.csl"), 1, "実ディスクを介さず seam 経由で 1 回だけ読むはず");
  }
}
