//! 参照定義ファイルの読み込み。
//!
//! TOML / JSON のトップレベルを、参照 ID をキーとする [`References`] として読み込む。
//! ファイル形式は拡張子で判別する。

mod date;
mod error;
mod name;
mod reference;

use std::{collections::HashMap, path::Path};

pub use date::{Date, DateCirca, DatePart, DateSeason};
pub use error::ReadReferencesError;
pub use name::Name;
pub use reference::{NumberOrString, Reference, ReferenceType, References};
use tracing::{debug, info};

/// 参照定義ファイルの形式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
  /// TOML 形式
  Toml,
  /// JSON 形式
  Json,
}

impl Format {
  /// 拡張子から形式を判定する。判別できない場合は `None` を返す。
  fn from_extension(path: &Path) -> Option<Self> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    return match ext.as_str() {
      "toml" => Some(Self::Toml),
      "json" => Some(Self::Json),
      _ => None,
    };
  }
}

/// 参照定義ファイルを読み込む。
///
/// `path` が `None` の場合は空の参照定義を返す。`path` は呼び出し元（`config::read_config`）が
/// 既に絶対化済みのものを渡す想定で、このクレート自身は相対パスの解決を行わない。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - 拡張子がサポートされていない場合
/// - TOML / JSON のパースに失敗した場合（著者名の排他性違反・空 / 重複 ID・未知フィールドを含む）
pub fn read_references<P: AsRef<Path>>(
  source: &dyn config::ProjectSource,
  path: Option<P>,
) -> Result<References, ReadReferencesError> {
  let Some(path) = path else {
    info!("参照定義ファイルが指定されていないため、空の参照定義を返します");
    return Ok(References(HashMap::new()));
  };
  let path_ref = path.as_ref();
  debug!(references_path = %path_ref.display(), "参照定義ファイルの読み込みを開始します");
  let content = source.read_text(&config::ProjectPath::new(path_ref)).map_err(|source| {
    return ReadReferencesError::ReadFile {
      path: path_ref.display().to_string(),
      source,
    };
  })?;
  let references = parse_references(&content, path_ref)?;
  let reference_count = references.len();
  info!(reference_count, "参照定義ファイルの読み込みが完了しました");
  return Ok(references);
}

/// テキストを [`References`] にパースします（I/O なし）。
///
/// `source_path` は形式判別とエラー表示だけに使い、ファイルシステムへはアクセスしない。
///
/// # Errors
///
/// - 拡張子がサポートされていない場合は [`ReadReferencesError::UnsupportedExtension`] を返します。
/// - TOML の構文・値が不正な場合は [`ReadReferencesError::ParseToml`] を返します。
/// - JSON の構文・値が不正な場合は [`ReadReferencesError::ParseJson`] を返します。
fn parse_references(text: &str, source_path: &Path) -> Result<References, ReadReferencesError> {
  let format = Format::from_extension(source_path).ok_or_else(|| {
    return ReadReferencesError::UnsupportedExtension {
      path: source_path.display().to_string(),
    };
  })?;
  return match format {
    Format::Toml => toml::from_str(text).map_err(|source| {
      return ReadReferencesError::ParseToml {
        path: source_path.display().to_string(),
        source,
      };
    }),
    Format::Json => serde_json::from_str(text).map_err(|source| {
      return ReadReferencesError::ParseJson {
        path: source_path.display().to_string(),
        source,
      };
    }),
  };
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use config::{FilesystemProjectSource, MemoryProjectSource, SourceReadError};

  use super::{
    DateCirca, DatePart, DateSeason, Name, NumberOrString, ReadReferencesError, parse_references, read_references,
  };

  /// `parse_references` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  /// JSON 用のダミーパス。
  fn dummy_json_source() -> &'static Path { return Path::new("test.json"); }

  /// 参照 ID をキーとするトップレベル JSON をそのまま返す（ラッパーテーブルは持たない）。
  fn json_doc(references_json: &str) -> String { return references_json.to_string(); }

  #[test]
  fn read_references_returns_empty_when_path_is_none() {
    // Arrange
    let source = FilesystemProjectSource::new();

    // Act
    let result: super::References = read_references::<&Path>(&source, None).unwrap();

    // Assert
    assert!(result.is_empty());
  }

  #[test]
  fn parse_references_fails_on_invalid_toml_syntax() {
    // Arrange / Act
    let result = parse_references("= \nthis is not valid toml", dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn parse_references_fails_on_empty_id() {
    // Arrange
    let toml = String::from(
      "[\"\"]\n\
       type = \"book\"\n\
       [[\"\".author]]\n\
       family = \"Doe\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    let Err(ReadReferencesError::ParseToml { source, .. }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    assert!(source.to_string().contains("空文字列"));
  }

  #[test]
  fn parse_references_fails_on_whitespace_only_id() {
    // Arrange
    let toml = String::from(
      "[\"  \"]\n\
       type = \"book\"\n\
       [[\"  \".author]]\n\
       family = \"Doe\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    let Err(ReadReferencesError::ParseToml { source, .. }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    assert!(source.to_string().contains("空白のみ"));
  }

  #[test]
  fn parse_references_fails_on_duplicate_toml_keys() {
    // Arrange
    let toml = String::from(
      "[dup]\n\
       type = \"book\"\n\
       [[dup.author]]\n\
       family = \"Doe\"\n\n\
       [dup]\n\
       type = \"book\"\n\
       [[dup.author]]\n\
       family = \"Roe\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn parse_references_reads_personal_author() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\
       given = \"John\"\n\
       suffix = \"Jr.\"\n",
    );

    // Act
    let refs = parse_references(&toml, dummy_source()).unwrap();

    // Assert
    let author = &refs.0["ref1"].author.as_ref().unwrap()[0];
    assert_eq!(
      *author,
      Name::Personal {
        family: "Doe".to_string(),
        given: Some("John".to_string()),
        dropping_particle: None,
        non_dropping_particle: None,
        suffix: Some("Jr.".to_string()),
      }
    );
  }

  #[test]
  fn parse_references_reads_organization_author() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       literal = \"ACME Corp\"\n",
    );

    // Act
    let refs = parse_references(&toml, dummy_source()).unwrap();

    // Assert
    let author = &refs.0["ref1"].author.as_ref().unwrap()[0];
    assert_eq!(
      *author,
      Name::Organization {
        literal: "ACME Corp".to_string(),
      }
    );
  }

  #[test]
  fn parse_references_fails_when_author_has_both_family_and_literal() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\
       literal = \"ACME Corp\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    let Err(ReadReferencesError::ParseToml { source, .. }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    assert!(source.to_string().contains("両方"));
  }

  #[test]
  fn parse_references_fails_when_author_has_neither_family_nor_literal() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       given = \"John\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn read_references_fails_on_read_file_error() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let path = PathBuf::from("/nonexistent/path/to/references.toml");

    // Act
    let result = read_references(&source, Some(&path));

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ReadFile { .. })));
  }

  #[test]
  fn read_references_reads_through_project_source() {
    // Arrange
    let source = MemoryProjectSource::new().with_text(
      "/project/references.toml",
      "[ref1]\n\
       type = \"book\"\n\
       title = \"Sample\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n",
    );
    let path = PathBuf::from("/project/references.toml");

    // Act
    let references = read_references(&source, Some(&path)).expect("有効な TOML は読み込めるはず");

    // Assert
    assert_eq!(references.len(), 1);
    assert!(references.contains_key("ref1"));
    assert_eq!(source.read_count("/project/references.toml"), 1, "実ディスクを介さず seam 経由で 1 回だけ読むはず");
  }

  #[test]
  fn read_references_reports_missing_file_via_source_read_error() {
    // Arrange
    let source = MemoryProjectSource::new();
    let path = PathBuf::from("/project/missing.toml");

    // Act
    let result = read_references(&source, Some(&path));

    // Assert
    let Err(ReadReferencesError::ReadFile { source, .. }) = result else {
      panic!("ReadFile を期待, got {result:?}");
    };
    assert!(matches!(source, SourceReadError::NotFound { .. }));
  }

  #[test]
  fn read_references_succeeds_with_valid_file() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.toml");
    std::fs::write(
      &references_path,
      "[ref1]\n\
       type = \"book\"\n\
       title = \"Sample Book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\
       given = \"John\"\n",
    )
    .unwrap();

    // Act
    let result = read_references(&source, Some(&references_path)).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    assert!(result.contains_key("ref1"));
  }

  #[test]
  fn parse_references_fails_on_invalid_json_syntax() {
    // Arrange / Act
    let result = parse_references("{ this is not valid json", dummy_json_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseJson { .. })));
  }

  #[test]
  fn parse_references_fails_on_unsupported_extension() {
    // Arrange / Act
    let result = parse_references("anything", Path::new("test.yaml"));

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::UnsupportedExtension { .. })));
  }

  #[test]
  fn parse_references_fails_on_empty_id_for_json() {
    // Arrange
    let json = json_doc("{\"\": {\"type\": \"book\", \"author\": [{\"family\": \"Doe\"}]}}");

    // Act
    let result = parse_references(&json, dummy_json_source());

    // Assert
    let Err(ReadReferencesError::ParseJson { source, .. }) = result else {
      panic!("expected ParseJson, got {result:?}");
    };
    assert!(source.to_string().contains("空文字列"));
  }

  #[test]
  fn read_references_succeeds_with_valid_json_file() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.json");
    let json = json_doc(
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"title\": \"Sample Book\", \
         \"issued\": {\"date-parts\": [[2024, 1, 15]]}, \
         \"author\": [{\"family\": \"Doe\", \"given\": \"John\"}]\
       }}",
    );
    std::fs::write(&references_path, json).unwrap();

    // Act
    let result = read_references(&source, Some(&references_path)).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let reference = result.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    let parts = issued.date_parts.as_ref().unwrap();
    assert_eq!(parts.len(), 1);
    assert!(matches!(
      parts[0].as_slice(),
      [
        DatePart::Number(2024),
        DatePart::Number(1),
        DatePart::Number(15)
      ]
    ));
  }

  #[test]
  fn read_references_parses_structured_date_in_toml() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.toml");
    std::fs::write(
      &references_path,
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\n\
       [ref1.issued]\n\
       date-parts = [[2024, 1, 15]]\n\
       circa = true\n\
       season = \"spring\"\n",
    )
    .unwrap();

    // Act
    let result = read_references(&source, Some(&references_path)).unwrap();

    // Assert
    let reference = result.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    let parts = issued.date_parts.as_ref().unwrap();
    assert_eq!(parts.len(), 1);
    assert!(matches!(
      parts[0].as_slice(),
      [
        DatePart::Number(2024),
        DatePart::Number(1),
        DatePart::Number(15)
      ]
    ));
    assert!(matches!(issued.circa, Some(DateCirca::Bool(true))));
    assert!(matches!(&issued.season, Some(DateSeason::String(s)) if s == "spring"));
  }

  #[test]
  fn parse_references_rejects_date_range_in_toml() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\n\
       [ref1.issued]\n\
       date-parts = [[2024, 1, 15], [2024, 12, 31]]\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    let Err(ReadReferencesError::ParseToml { source, .. }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    assert!(source.to_string().contains("日付範囲"));
  }

  #[test]
  fn parse_references_rejects_date_range_in_json() {
    // Arrange
    let json = json_doc(
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"issued\": {\"date-parts\": [[2024, 1, 15], [2024, 12, 31]]}, \
         \"author\": [{\"family\": \"Doe\"}]\
       }}",
    );

    // Act
    let result = parse_references(&json, dummy_json_source());

    // Assert
    let Err(ReadReferencesError::ParseJson { source, .. }) = result else {
      panic!("expected ParseJson, got {result:?}");
    };
    assert!(source.to_string().contains("日付範囲"));
  }

  #[test]
  fn read_references_parses_structured_date_in_json() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.json");
    let json = json_doc(
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"issued\": {\
           \"date-parts\": [[2024, 1, 15]], \
           \"season\": 1, \
           \"circa\": true, \
           \"literal\": \"early 2024\", \
           \"raw\": \"Jan 15, 2024\"\
         }, \
         \"author\": [{\"family\": \"Doe\"}]\
       }}",
    );
    std::fs::write(&references_path, json).unwrap();

    // Act
    let result = read_references(&source, Some(&references_path)).unwrap();

    // Assert
    let reference = result.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    let parts = issued.date_parts.as_ref().unwrap();
    assert!(matches!(
      parts[0].as_slice(),
      [
        DatePart::Number(2024),
        DatePart::Number(1),
        DatePart::Number(15)
      ]
    ));
    assert!(matches!(issued.season, Some(DateSeason::Number(1))));
    assert!(matches!(issued.circa, Some(DateCirca::Bool(true))));
    assert_eq!(issued.literal.as_deref(), Some("early 2024"));
    assert_eq!(issued.raw.as_deref(), Some("Jan 15, 2024"));
  }

  #[test]
  fn read_references_fails_on_unsupported_extension_file() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.yaml");
    std::fs::write(&references_path, b"anything: true").unwrap();

    // Act
    let result = read_references(&source, Some(&references_path));

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::UnsupportedExtension { .. })));
  }

  #[test]
  fn read_references_accepts_number_variables_as_integers_and_strings_in_toml() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       volume = 3\n\
       edition = 2.5\n\
       page = \"1-10\"\n\
       issue = 7\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n",
    );

    // Act
    let refs = parse_references(&toml, dummy_source()).unwrap();
    let reference = refs.get("ref1").unwrap();

    // Assert
    assert!(matches!(reference.volume, Some(NumberOrString::Integer(3))));
    assert!(matches!(reference.edition, Some(NumberOrString::Float(value)) if (value - 2.5).abs() < f64::EPSILON));
    assert!(matches!(&reference.page, Some(NumberOrString::String(value)) if value == "1-10"));
    assert!(matches!(reference.issue, Some(NumberOrString::Integer(7))));
  }

  #[test]
  fn read_references_accepts_number_variables_as_integers_and_strings_in_json() {
    // Arrange
    let json = json_doc(
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"volume\": 3, \
         \"edition\": 2.5, \
         \"page\": \"1-10\", \
         \"issue\": \"S2\", \
         \"author\": [{\"family\": \"Doe\"}]\
       }}",
    );

    // Act
    let refs = parse_references(&json, dummy_json_source()).unwrap();
    let reference = refs.get("ref1").unwrap();

    // Assert
    assert!(matches!(reference.volume, Some(NumberOrString::Integer(3))));
    assert!(matches!(reference.edition, Some(NumberOrString::Float(value)) if (value - 2.5).abs() < f64::EPSILON));
    assert!(matches!(&reference.page, Some(NumberOrString::String(value)) if value == "1-10"));
    assert!(matches!(&reference.issue, Some(NumberOrString::String(value)) if value == "S2"));
  }

  #[test]
  fn parse_references_rejects_unknown_field_in_reference() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       unknown_field = \"oops\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn parse_references_rejects_unknown_field_in_name() {
    // Arrange
    let toml = String::from(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\
       unknown_name_field = \"oops\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn parse_references_rejects_non_table_top_level_value() {
    // Arrange
    let toml = "unexpected = true\n";

    // Act
    let result = parse_references(toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn parse_references_rejects_unknown_date_field() {
    // Arrange
    let json = json_doc(
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"issued\": {\"date-parts\": [[2024]], \"bogus\": 1}, \
         \"author\": [{\"family\": \"Doe\"}]\
       }}",
    );

    // Act
    let result = parse_references(&json, dummy_json_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseJson { .. })));
  }

  #[test]
  fn parse_references_rejects_duplicate_json_keys() {
    // Arrange
    let json = json_doc(
      "{\"dup\": {\"type\": \"book\", \"author\": [{\"family\": \"Doe\"}]}, \
        \"dup\": {\"type\": \"book\", \"author\": [{\"family\": \"Roe\"}]}}",
    );

    // Act
    let result = parse_references(&json, dummy_json_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseJson { .. })));
  }
}
