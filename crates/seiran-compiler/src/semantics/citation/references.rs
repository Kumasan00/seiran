//! 参照定義ファイルの読み込み。
//!
//! TOML / JSON のトップレベルを、参照 ID をキーとする [`References`] として読み込む。
//! ファイル形式は拡張子で判別する。

mod date;
mod error;
mod name;
mod reference;

use std::{collections::HashMap, path::Path};

// `Date` / `Name` / `ReferenceType` / `NumberOrString` は [`Reference`] のフィールド型として
// 生きているが、名前を再エクスポートする必要はない（外から名指しする消費者がいない）。
pub(crate) use error::ReadReferencesError;
pub(crate) use reference::{Reference, References};
use tracing::debug;

use crate::project::{ProjectPath, ProjectSource};

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
/// `path` が `None` の場合は空の参照定義を返す。`path` は `project::config::load` が `PathResolver` で
/// 解決済みの `ProjectPath` を渡す想定で、このクレート自身は相対パスの解決を行わない。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - 拡張子がサポートされていない場合
/// - TOML / JSON のパースに失敗した場合（著者名の排他性違反・空 / 重複 ID・未知フィールドを含む）
pub(crate) fn read_references(
  source: &dyn ProjectSource,
  path: Option<&ProjectPath>,
) -> Result<References, ReadReferencesError> {
  let Some(path) = path else {
    debug!("参照定義ファイル未指定のため空の参照定義を使用");
    return Ok(References(HashMap::new()));
  };
  let content = source.read_text(path).map_err(|source| {
    return ReadReferencesError::ReadFile {
      path: path.to_string(),
      source,
    };
  })?;
  let references = parse_references(&content, path.as_ref())?;
  let reference_count = references.len();
  debug!(references_path = %path, reference_count, "参照定義ファイルを読込");
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
  use std::path::Path;

  use super::{
    ReadReferencesError,
    date::{DatePart, Season},
    name::Name,
    parse_references, read_references,
    reference::NumberOrString,
  };
  use crate::project::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, SourceReadError};

  /// `parse_references` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  /// JSON 用のダミーパス。
  fn dummy_json_source() -> &'static Path { return Path::new("test.json"); }

  /// 参照 ID をキーとするトップレベル JSON をそのまま返す（ラッパーテーブルは持たない）。
  fn json_doc(references_json: &str) -> String { return references_json.to_string(); }

  /// `[ref1.issued]` テーブルの本体だけを差し替えた TOML を解析し、`ParseToml` の元エラー文言を返す。
  fn issued_toml_error(issued_body: &str) -> String {
    let toml = format!(
      "[ref1]\n\
       type = \"book\"\n\
       [[ref1.author]]\n\
       family = \"Doe\"\n\n\
       [ref1.issued]\n\
       {issued_body}\n"
    );
    let result = parse_references(&toml, dummy_source());
    let Err(ReadReferencesError::ParseToml { source, .. }) = result else {
      panic!("expected ParseToml, got {result:?}");
    };
    return source.to_string();
  }

  /// `issued` の JSON 値だけを差し替えた JSON を解析し、`ParseJson` の元エラー文言を返す。
  fn issued_json_error(issued_json: &str) -> String {
    let json = json_doc(&format!(
      "{{\"ref1\": {{\"type\": \"book\", \"issued\": {issued_json}, \"author\": [{{\"family\": \"Doe\"}}]}}}}"
    ));
    let result = parse_references(&json, dummy_json_source());
    let Err(ReadReferencesError::ParseJson { source, .. }) = result else {
      panic!("expected ParseJson, got {result:?}");
    };
    return source.to_string();
  }

  #[test]
  fn read_references_returns_empty_when_path_is_none() {
    // Arrange
    let source = FilesystemProjectSource;

    // Act
    let result: super::References = read_references(&source, None).unwrap();

    // Assert
    assert!(result.is_empty());
  }

  #[test]
  fn parse_references_fails_on_invalid_toml_syntax() {
    let result = parse_references("= \nthis is not valid toml", dummy_source());

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
    let source = FilesystemProjectSource;
    let path = ProjectPath::new("/nonexistent/path/to/references.toml");

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
    let path = ProjectPath::new("/project/references.toml");

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
    let path = ProjectPath::new("/project/missing.toml");

    // Act
    let result = read_references(&source, Some(&path));

    // Assert
    let Err(ReadReferencesError::ReadFile { source, .. }) = result else {
      panic!("ReadFile を期待, got {result:?}");
    };
    assert!(matches!(source, SourceReadError::NotFound), "未登録パスは NotFound になるはず: {source:?}");
  }

  #[test]
  fn read_references_succeeds_with_valid_file() {
    // Arrange
    let source = FilesystemProjectSource;
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
    let result = read_references(&source, Some(&ProjectPath::new(&references_path))).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    assert!(result.contains_key("ref1"));
  }

  #[test]
  fn parse_references_fails_on_invalid_json_syntax() {
    let result = parse_references("{ this is not valid json", dummy_json_source());

    assert!(matches!(result, Err(ReadReferencesError::ParseJson { .. })));
  }

  #[test]
  fn parse_references_fails_on_unsupported_extension() {
    let result = parse_references("anything", Path::new("test.yaml"));

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
    let source = FilesystemProjectSource;
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
    let result = read_references(&source, Some(&ProjectPath::new(&references_path))).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    let reference = result.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    assert!(matches!(
      issued.parts.as_slice(),
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
    let source = FilesystemProjectSource;
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
       circa = true\n",
    )
    .unwrap();

    // Act
    let result = read_references(&source, Some(&ProjectPath::new(&references_path))).unwrap();

    // Assert
    let reference = result.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    assert!(matches!(
      issued.parts.as_slice(),
      [
        DatePart::Number(2024),
        DatePart::Number(1),
        DatePart::Number(15)
      ]
    ));
    assert_eq!(issued.circa, Some(true));
    assert_eq!(issued.season, None);
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
  fn parse_references_rejects_raw_date_in_toml() {
    let message = issued_toml_error("raw = \"2014-05-01\"");

    // 診断はキー名を言い、toml のスニペットが該当エントリの日付テーブルを指す
    assert!(message.contains("`raw`"), "{message}");
    assert!(message.contains("`date-parts`"), "{message}");
    assert!(message.contains("[ref1.issued]"), "{message}");
  }

  #[test]
  fn parse_references_rejects_literal_date_in_toml() {
    let message = issued_toml_error("literal = \"circa 1900\"");

    assert!(message.contains("`literal`"), "{message}");
    assert!(message.contains("[ref1.issued]"), "{message}");
  }

  #[test]
  fn parse_references_rejects_literal_alongside_date_parts() {
    // `date-parts` があっても `literal` は整形器に読まれないので黙って捨てずに拒否する
    let message = issued_toml_error("date-parts = [[2014]]\nliteral = \"early 2014\"");

    assert!(message.contains("`literal`"), "{message}");
  }

  #[test]
  fn parse_references_rejects_raw_and_literal_in_json() {
    for key in ["raw", "literal"] {
      let message = issued_json_error(&format!("{{\"{key}\": \"2014\"}}"));

      // JSON はスニペットを持たないので、キー名と行・列で位置を示す
      assert!(message.contains(&format!("`{key}`")), "{message}");
      assert!(message.contains("line"), "{message}");
    }
  }

  #[test]
  fn parse_references_rejects_date_without_date_parts() {
    for body in ["season = 1", "circa = true"] {
      let message = issued_toml_error(body);

      assert!(message.contains("`date-parts` が必要"), "{body}: {message}");
    }
    let message = issued_json_error("{}");
    assert!(message.contains("`date-parts` が必要"), "{message}");
  }

  #[test]
  fn parse_references_rejects_duplicate_date_key_in_json() {
    // JSON は重複キーを構文で拒否しないので、後勝ちで先の値を黙って捨てずに拒否する
    for key in ["date-parts", "season", "circa"] {
      let value = match key {
        "date-parts" => "[[2014]]",
        "season" => "1",
        _ => "true",
      };
      let message = issued_json_error(&format!("{{\"date-parts\": [[2014]], \"{key}\": {value}, \"{key}\": {value}}}"));

      assert!(message.contains(&format!("`{key}`")), "{key}: {message}");
      assert!(message.contains("duplicate"), "{key}: {message}");
    }
  }

  #[test]
  fn parse_references_rejects_empty_date_parts() {
    let message = issued_toml_error("date-parts = []");

    assert!(message.contains("`date-parts` が空"), "{message}");
  }

  #[test]
  fn parse_references_rejects_date_parts_with_wrong_arity() {
    // 要素 0 個は整形器の内部で panic し、4 個目以降は黙って捨てられる
    for body in ["date-parts = [[]]", "date-parts = [[2024, 1, 15, 3]]"] {
      let message = issued_toml_error(body);

      assert!(message.contains("1〜3 要素"), "{body}: {message}");
    }
  }

  #[test]
  fn parse_references_rejects_empty_string_date_part() {
    // 整形器は空文字列の要素を捨てるので、`[[""]]` は要素 0 個と同じく panic し、
    // `[["", 2024]]` は黙って年がずれる
    for body in ["date-parts = [[\"\"]]", "date-parts = [[\"\", 2024]]"] {
      let message = issued_toml_error(body);

      assert!(message.contains("空文字列"), "{body}: {message}");
    }
  }

  #[test]
  fn parse_references_accepts_season_numbers_on_year_only_date() {
    for (number, expected) in [
      (1, Season::Spring),
      (2, Season::Summer),
      (3, Season::Autumn),
      (4, Season::Winter),
    ] {
      // Arrange
      let toml = format!(
        "[ref1]\n\
         type = \"book\"\n\
         [ref1.issued]\n\
         date-parts = [[2014]]\n\
         season = {number}\n"
      );

      // Act
      let references = parse_references(&toml, dummy_source()).unwrap();

      // Assert
      let issued = references.get("ref1").unwrap().issued.as_ref().unwrap();
      assert_eq!(issued.season, Some(expected), "season = {number}");
    }
  }

  #[test]
  fn parse_references_rejects_season_other_than_integer_one_to_four() {
    // 整形器は 1〜4 の整数（と数値文字列）しか季節として読まず、他は黙って捨てる。
    // 数値文字列 `"1"` も別綴りなので拒否する（#741: 1 綴りだけ受理）
    for value in ["\"spring\"", "\"1\"", "0", "5", "-1", "1.0"] {
      let message = issued_toml_error(&format!("date-parts = [[2014]]\nseason = {value}"));

      assert!(message.contains("`season`"), "{value}: {message}");
      assert!(message.contains("1〜4"), "{value}: {message}");
    }
  }

  #[test]
  fn parse_references_rejects_string_season_in_json() {
    let message = issued_json_error("{\"date-parts\": [[2014]], \"season\": \"spring\"}");

    // JSON はスニペットを持たないので、文言のキー名で位置を示す
    assert!(message.contains("`season`"), "{message}");
    assert!(message.contains("line"), "{message}");
  }

  #[test]
  fn parse_references_rejects_season_on_date_with_month() {
    // 整形器は月があると季節を描画しないので、月付きの日付への季節は黙って捨てずに拒否する。
    // キー順に依らない（`season` が `date-parts` より前でも拒否する）
    for body in [
      "date-parts = [[2014, 5]]\nseason = 2",
      "date-parts = [[2014, 5, 1]]\nseason = 2",
      "season = 2\ndate-parts = [[2014, 5]]",
    ] {
      let message = issued_toml_error(body);

      assert!(message.contains("`season`"), "{body}: {message}");
      assert!(message.contains("月の無い日付"), "{body}: {message}");
    }
  }

  #[test]
  fn parse_references_accepts_false_circa() {
    // Arrange
    let toml = "[ref1]\n\
                type = \"book\"\n\
                [ref1.issued]\n\
                date-parts = [[2014]]\n\
                circa = false\n";

    // Act
    let references = parse_references(toml, dummy_source()).unwrap();

    // Assert
    let issued = references.get("ref1").unwrap().issued.as_ref().unwrap();
    assert_eq!(issued.circa, Some(false));
  }

  #[test]
  fn parse_references_rejects_non_bool_circa_in_toml() {
    // 整形器は `"true"` / `1` を真、それ以外を黙って偽とする。別綴りも含めて真偽値以外は拒否する（#741）
    for value in ["\"yes\"", "\"true\"", "1", "0", "2"] {
      let message = issued_toml_error(&format!("date-parts = [[2014]]\ncirca = {value}"));

      assert!(message.contains("`circa`"), "{value}: {message}");
      assert!(message.contains("真偽値"), "{value}: {message}");
    }
  }

  #[test]
  fn parse_references_rejects_non_bool_circa_in_json() {
    for value in ["1", "\"true\"", "\"yes\""] {
      let message = issued_json_error(&format!("{{\"date-parts\": [[2014]], \"circa\": {value}}}"));

      assert!(message.contains("`circa`"), "{value}: {message}");
      assert!(message.contains("line"), "{value}: {message}");
    }
  }

  #[test]
  fn read_references_parses_structured_date_in_json() {
    // Arrange
    let source = FilesystemProjectSource;
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.json");
    let json = json_doc(
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"issued\": {\
           \"date-parts\": [[2024]], \
           \"season\": 1, \
           \"circa\": true\
         }, \
         \"author\": [{\"family\": \"Doe\"}]\
       }}",
    );
    std::fs::write(&references_path, json).unwrap();

    // Act
    let result = read_references(&source, Some(&ProjectPath::new(&references_path))).unwrap();

    // Assert
    let reference = result.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    assert!(matches!(issued.parts.as_slice(), [DatePart::Number(2024)]));
    assert_eq!(issued.season, Some(Season::Spring));
    assert_eq!(issued.circa, Some(true));
  }

  #[test]
  fn read_references_fails_on_unsupported_extension_file() {
    // Arrange
    let source = FilesystemProjectSource;
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.yaml");
    std::fs::write(&references_path, b"anything: true").unwrap();

    // Act
    let result = read_references(&source, Some(&ProjectPath::new(&references_path)));

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
