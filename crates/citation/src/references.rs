//! 参照定義ファイルの読み込みモジュール
//!
//! `citation` クレート内部の実装詳細で、公開する型は crate root で再エクスポートし
//! `citation::Reference` のように参照する（`citation::references::Reference` は使わない）。
//!
//! TOML / JSON 形式の参照定義ファイルを読み込み、`id` をキーとする参照定義のマップを返す。参照定義は
//! ファイルのトップレベルそのものを keyed-table 形式（テーブルキーが参照 ID）で記述する（`references.`
//! 接頭辞は不要で、`[kwan2014]` のように直接エントリを書く）。
//!
//! 著者名は確定型 [`Name`]（`Personal` / `Organization` の 2 択 enum）として直接デシリアライズする。
//! `family`（個人著者）と `literal`（組織著者）の同時指定／不指定という不正状態は、[`Name`] の手書き
//! [`Deserialize`](serde::Deserialize) 実装が検証して fail-fast に拒否するため、確定後の型には表現でき
//! なくなる。空（空白のみを含む）・重複の参照 ID も同様にデシリアライズ時点で拒否する（TOML はパーサ段階、
//! JSON は専用デシリアライザで検出）。未知のフィールドは `deny_unknown_fields` で拒否し、日付オブジェクト・
//! name オブジェクトの未知キーも同様にエラーとする（タイポ検出のため、CSL 前方互換より厳格性を優先）。
//! 日付範囲（`date-parts` の 2 要素指定）は CSL-JSN 担体経由の hayagriva が未対応のため拒否する。
//! これらの値検証エラーはいずれも TOML / JSON の解析エラー（[`ReadReferencesError::ParseToml`] /
//! [`ReadReferencesError::ParseJson`]）として、ソース上の位置情報付きで報告される。
//!
//! [`Reference`] は CSL-JSN（kebab-case キー）へ [`Serialize`](serde::Serialize) でき、
//! [`crate::bridge`] が hayagriva の担体 `citationberg::json::Item` を組み立てる出力経路に使う。
//!
//! ファイル形式は拡張子 (`.toml` / `.json`) で判別する。
//!
//! 型定義はサブモジュール（`error` / `date` / `name` / `reference`）に分割し、本モジュールはそれらを
//! 再エクスポートしつつ、読み込み・パースのオーケストレーションを担う。

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
/// # Arguments
///
/// * `path` - 参照定義ファイル（`.toml` / `.json`）のパス。`None` の場合は空の参照定義を返す。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - 拡張子がサポートされていない場合
/// - TOML / JSON のパースに失敗した場合（著者名の排他性違反・空 / 重複 ID・未知フィールドを含む）
pub fn read_references<P: AsRef<Path>>(path: Option<P>) -> Result<References, ReadReferencesError> {
  let Some(path) = path else {
    info!("参照定義ファイルが指定されていないため、空の参照定義を返します");
    return Ok(References(HashMap::new()));
  };
  let path_ref = path.as_ref();
  debug!(references_path = %path_ref.display(), "参照定義ファイルの読み込みを開始します");
  let content = std::fs::read_to_string(path_ref).map_err(|source| ReadReferencesError::ReadFile {
    path: path_ref.display().to_string(),
    source,
  })?;
  let references = parse_references(&content, path_ref)?;
  let reference_count = references.len();
  info!(reference_count, "参照定義ファイルの読み込みが完了しました");
  return Ok(references);
}

/// テキストを [`References`] にパースします（I/O なし）。
///
/// `source_path` の拡張子（`.toml` / `.json`）から形式を判別します。エラー報告に使う表示用パスでもあり、
/// ファイルシステムへのアクセスには使われません。著者名の排他性・空 / 重複 ID・未知フィールドといった値
/// 検証は、各型のデシリアライザがパース時点で実施します。
///
/// # Errors
///
/// - 拡張子がサポートされていない場合は [`ReadReferencesError::UnsupportedExtension`] を返します。
/// - TOML の構文・値が不正な場合は [`ReadReferencesError::ParseToml`] を返します。
/// - JSON の構文・値が不正な場合は [`ReadReferencesError::ParseJson`] を返します。
fn parse_references(text: &str, source_path: &Path) -> Result<References, ReadReferencesError> {
  let format = Format::from_extension(source_path).ok_or_else(|| ReadReferencesError::UnsupportedExtension {
    path: source_path.display().to_string(),
  })?;
  return match format {
    Format::Toml => toml::from_str(text).map_err(|source| ReadReferencesError::ParseToml {
      path: source_path.display().to_string(),
      source,
    }),
    Format::Json => serde_json::from_str(text).map_err(|source| ReadReferencesError::ParseJson {
      path: source_path.display().to_string(),
      source,
    }),
  };
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

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
    // Arrange / Act
    let result: super::References = read_references::<&Path>(None).unwrap();

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
    // Arrange: 空文字列の参照 ID はデシリアライズ時に拒否される
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
    // Arrange: 空白のみの参照 ID
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
    // Arrange: keyed-table 形式では重複 ID は同一テーブルの再定義となり TOML パースエラーになる
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
    // Arrange: family のみ → 個人著者
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
    // Arrange: literal のみ → 組織著者
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
    // Arrange: family/literal 同時指定は排他性違反
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
    // Arrange: given のみ（family も literal もない）
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
    let path = PathBuf::from("/nonexistent/path/to/references.toml");

    // Act
    let result = read_references(Some(&path));

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ReadFile { .. })));
  }

  #[test]
  fn read_references_succeeds_with_valid_file() {
    // Arrange
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
    let result = read_references(Some(&references_path)).unwrap();

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
    let result = read_references(Some(&references_path)).unwrap();

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
    // Arrange — 単一日付（範囲はサポートしない）
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
    let result = read_references(Some(&references_path)).unwrap();

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
    // Arrange — date-parts 2 要素（日付範囲）は CSL-JSN 担体が未対応のため拒否される
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
    let result = read_references(Some(&references_path)).unwrap();

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
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.yaml");
    std::fs::write(&references_path, b"anything: true").unwrap();

    // Act
    let result = read_references(Some(&references_path));

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
    // Arrange: Reference に未知フィールドを含める
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
    // Arrange: 著者名に未知フィールドを含める
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
    // Arrange: トップレベルの値はすべて参照テーブルでなければならない（スカラー値は Reference に
    // デシリアライズできず拒否される）
    let toml = "unexpected = true\n";

    // Act
    let result = parse_references(toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn parse_references_rejects_unknown_date_field() {
    // Arrange: 日付オブジェクトに未知キーを含める
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
    // Arrange: JSON はキー重複を許すが、Seiran は後勝ちを許さず厳格に拒否する
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
