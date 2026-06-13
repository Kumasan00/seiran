//! 参照定義ファイルの読み込みモジュール
//!
//! TOML / JSON 形式の参照定義ファイルを読み込み、検証・正規化を経て `id` をキーとする参照定義の
//! マップを返す。参照定義は keyed-table 形式（テーブルキーが参照 ID）で記述する。
//!
//! 著者は serde 境界では [`RawName`]（全フィールド任意）として受理し、検証フェーズで
//! [`Name`]（`Personal` / `Organization` の 2 択 enum）へ変換する。これにより `family`（個人著者）と
//! `literal`（組織著者）の同時指定／不指定という不正状態は、変換後の型には表現できなくなる。
//! 著者の判別不能・空の参照 ID・`style_path` の解決失敗は、すべて `MultipleValidationErrors` に
//! 集約して 1 度に報告する。参照 ID の重複は、TOML ではパース時にエラーとなり、JSON では後勝ちで
//! 解決する。
//!
//! ファイル形式は拡張子 (`.toml` / `.json`) で判別する。
//!
//! 型定義はサブモジュール（`error` / `date` / `name` / `reference`）に分割し、本モジュールはそれらを
//! 再エクスポートしつつ、読み込み・パース・検証正規化のオーケストレーションを担う。

mod date;
mod error;
mod name;
mod reference;

use std::{
  collections::HashMap,
  path::{Path, PathBuf},
};

pub use date::{Date, DateCirca, DatePart, DateSeason};
pub use error::{ReadReferencesError, ValidationError};
pub use name::{Name, NameError, RawName};
pub use reference::{NumberOrString, Reference, ReferenceType, References};
use tracing::info;

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

/// 参照定義ファイルを読み込む利便ラッパ
///
/// [`parse_references`] → [`resolve`] を連結します。生産コードはこの関数を使用します。
///
/// # Arguments
///
/// * `path` - 参照定義ファイル（`.toml` / `.json`）のパス。`None` の場合は空の参照定義を返す。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - TOML / JSON のパースに失敗した場合
/// - 拡張子がサポートされていない場合
/// - フィールド単位の検証・空の参照 ID 検出・`style_path` の解決に失敗した場合
pub fn read_references<P: AsRef<Path>>(path: Option<P>) -> Result<References, ReadReferencesError> {
  let Some(path) = path else {
    info!("参照定義ファイルが指定されていないため、空の参照定義を返します");
    return Ok(References {
      style_path: PathBuf::new(),
      references: HashMap::new(),
    });
  };
  let path_ref = path.as_ref();
  info!(references_path = %path_ref.display(), "参照定義ファイルの読み込みを開始します");
  let content = std::fs::read_to_string(path_ref).map_err(|source| ReadReferencesError::ReadFile {
    path: path_ref.display().to_string(),
    source,
  })?;
  let parsed = parse_references(&content, path_ref)?;
  let references = resolve(parsed)?;
  let count = references.references.len();
  info!(count, "参照定義ファイルの読み込みが完了しました");
  return Ok(references);
}

/// テキストを `References<RawName>` にパースします（I/O なし、検証・正規化なし）。
///
/// 著者名は検証前の [`RawName`] のまま受理します。`source_path` の拡張子（`.toml` / `.json`）から
/// 形式を判別します。エラー報告に使う表示用パスでもあり、ファイルシステムへのアクセスには使われません。
/// 値検証・`style_path` の正規化は行いません。検証・正規化は [`resolve`] で実行します。
///
/// # Errors
///
/// - 拡張子がサポートされていない場合は [`ReadReferencesError::UnsupportedExtension`] を返します。
/// - TOML の構文が不正な場合は [`ReadReferencesError::ParseToml`] を返します。
/// - JSON の構文が不正な場合は [`ReadReferencesError::ParseJson`] を返します。
fn parse_references(text: &str, source_path: &Path) -> Result<References<RawName>, ReadReferencesError> {
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

/// パース済みの `References<RawName>` を検証し、著者名を [`Name`] へ変換、`style_path` を
/// 正規化して `References<Name>` に確定します。
///
/// 空 ID チェック・著者名の検証変換・`style_path` の解決エラーを、すべて 1 回の
/// [`ReadReferencesError::MultipleValidationErrors`] に集約します（途中でエラーが出ても短絡せず、
/// すべての違反を収集してから報告します）。`references` は keyed-table 形式により既に id をキーと
/// するマップなので、`style_path` のみを更新します。
///
/// # Errors
///
/// 著者名の検証変換・空 ID・`style_path` の正規化のいずれかに違反があった場合は
/// [`ReadReferencesError::MultipleValidationErrors`] を返します。
fn resolve(refs: References<RawName>) -> Result<References<Name>, ReadReferencesError> {
  let mut errors: Vec<ValidationError> = Vec::new();
  validate_non_empty_ids(&refs.references, &mut errors);
  let references = convert_references(refs.references, &mut errors);
  let style_path = match refs.style_path.canonicalize() {
    Ok(canon) => Some(canon),
    Err(source) => {
      errors.push(ValidationError::StylePathResolution {
        path: refs.style_path.display().to_string(),
        source,
      });
      None
    },
  };

  if !errors.is_empty() {
    return Err(ReadReferencesError::MultipleValidationErrors { errors });
  }

  return Ok(References {
    style_path: style_path.expect("errors が空なら style_path は Some である"),
    references,
  });
}

/// 参照定義マップの各著者名を [`RawName`] から [`Name`] へ変換します（I/O なし）。
///
/// 変換に失敗した著者は結果から除外し、違反を [`ValidationError::Field`] として `errors` に追加します。
/// 戻り値は変換に成功した分のみを保持する「ベストエフォート」のマップで、`errors` が空のときにのみ
/// 確定値として採用されます。
fn convert_references(
  references: HashMap<String, Reference<RawName>>,
  errors: &mut Vec<ValidationError>,
) -> HashMap<String, Reference<Name>> {
  let mut converted = HashMap::with_capacity(references.len());
  for (id, reference) in references {
    let reference = convert_reference(reference, &id, errors);
    converted.insert(id, reference);
  }
  return converted;
}

/// 1 件の [`Reference`] について、著者名フィールドのみ [`RawName`] → [`Name`] に変換します。
///
/// 著者名以外のフィールドはそのまま移送します。変換失敗は `errors` に集約されます。
fn convert_reference(raw: Reference<RawName>, id: &str, errors: &mut Vec<ValidationError>) -> Reference<Name> {
  return Reference {
    reference_type: raw.reference_type,
    item_abstract: raw.item_abstract,
    annote: raw.annote,
    archive: raw.archive,
    archive_collection: raw.archive_collection,
    archive_location: raw.archive_location,
    archive_place: raw.archive_place,
    authority: raw.authority,
    call_number: raw.call_number,
    citation_key: raw.citation_key,
    citation_label: raw.citation_label,
    collection_title: raw.collection_title,
    container_title: raw.container_title,
    container_title_short: raw.container_title_short,
    dimensions: raw.dimensions,
    division: raw.division,
    doi: raw.doi,
    event: raw.event,
    event_title: raw.event_title,
    event_place: raw.event_place,
    genre: raw.genre,
    isbn: raw.isbn,
    issn: raw.issn,
    jurisdiction: raw.jurisdiction,
    keyword: raw.keyword,
    language: raw.language,
    license: raw.license,
    medium: raw.medium,
    note: raw.note,
    original_publisher: raw.original_publisher,
    original_publisher_place: raw.original_publisher_place,
    original_title: raw.original_title,
    part_title: raw.part_title,
    pmcid: raw.pmcid,
    pmid: raw.pmid,
    publisher: raw.publisher,
    publisher_place: raw.publisher_place,
    references: raw.references,
    reviewed_genre: raw.reviewed_genre,
    reviewed_title: raw.reviewed_title,
    scale: raw.scale,
    source: raw.source,
    status: raw.status,
    title: raw.title,
    title_short: raw.title_short,
    url: raw.url,
    volume_title: raw.volume_title,
    year_suffix: raw.year_suffix,
    chapter_number: raw.chapter_number,
    citation_number: raw.citation_number,
    collection_number: raw.collection_number,
    edition: raw.edition,
    first_reference_note_number: raw.first_reference_note_number,
    issue: raw.issue,
    locator: raw.locator,
    number: raw.number,
    number_of_pages: raw.number_of_pages,
    number_of_volumes: raw.number_of_volumes,
    page: raw.page,
    page_first: raw.page_first,
    part_number: raw.part_number,
    printing_number: raw.printing_number,
    section: raw.section,
    supplement_number: raw.supplement_number,
    version: raw.version,
    volume: raw.volume,
    accessed: raw.accessed,
    available_date: raw.available_date,
    event_date: raw.event_date,
    issued: raw.issued,
    original_date: raw.original_date,
    submitted: raw.submitted,
    author: convert_names(raw.author, id, "author", errors),
    chair: convert_names(raw.chair, id, "chair", errors),
    collection_editor: convert_names(raw.collection_editor, id, "collection-editor", errors),
    compiler: convert_names(raw.compiler, id, "compiler", errors),
    composer: convert_names(raw.composer, id, "composer", errors),
    container_author: convert_names(raw.container_author, id, "container-author", errors),
    contributor: convert_names(raw.contributor, id, "contributor", errors),
    curator: convert_names(raw.curator, id, "curator", errors),
    director: convert_names(raw.director, id, "director", errors),
    editor: convert_names(raw.editor, id, "editor", errors),
    editorial_director: convert_names(raw.editorial_director, id, "editorial-director", errors),
    editor_translator: convert_names(raw.editor_translator, id, "editor-translator", errors),
    executive_producer: convert_names(raw.executive_producer, id, "executive-producer", errors),
    guest: convert_names(raw.guest, id, "guest", errors),
    host: convert_names(raw.host, id, "host", errors),
    illustrator: convert_names(raw.illustrator, id, "illustrator", errors),
    interviewer: convert_names(raw.interviewer, id, "interviewer", errors),
    narrator: convert_names(raw.narrator, id, "narrator", errors),
    organizer: convert_names(raw.organizer, id, "organizer", errors),
    original_author: convert_names(raw.original_author, id, "original-author", errors),
    performer: convert_names(raw.performer, id, "performer", errors),
    producer: convert_names(raw.producer, id, "producer", errors),
    recipient: convert_names(raw.recipient, id, "recipient", errors),
    reviewed_author: convert_names(raw.reviewed_author, id, "reviewed-author", errors),
    script_writer: convert_names(raw.script_writer, id, "script-writer", errors),
    series_creator: convert_names(raw.series_creator, id, "series-creator", errors),
    translator: convert_names(raw.translator, id, "translator", errors),
  };
}

/// 1 つの著者名フィールド（`Option<Vec<RawName>>`）を `Option<Vec<Name>>` に変換します。
///
/// 各要素を [`Name::try_from`] で検証変換し、失敗した要素は除外して違反を
/// [`ValidationError::Field`]（パス例: `references.ref1.author[0]`）として `errors` に追加します。
fn convert_names(
  raw: Option<Vec<RawName>>,
  id: &str,
  field: &str,
  errors: &mut Vec<ValidationError>,
) -> Option<Vec<Name>> {
  let raw = raw?;
  let mut names = Vec::with_capacity(raw.len());
  for (index, name) in raw.into_iter().enumerate() {
    match Name::try_from(name) {
      Ok(name) => names.push(name),
      Err(error) => errors.push(ValidationError::Field {
        path: format!("references.{id}.{field}[{index}]"),
        message: error.to_string(),
      }),
    }
  }
  return Some(names);
}

/// 参照 ID（テーブルキー）が空でないことを検証し、違反を `errors` に追加する。
///
/// keyed-table 形式では id はマップのキーであり、値（`Reference`）の検証とは別に、キーの非空
/// チェックを明示的に行う。
fn validate_non_empty_ids(references: &HashMap<String, Reference<RawName>>, errors: &mut Vec<ValidationError>) {
  if references.contains_key("") {
    errors.push(ValidationError::Field {
      path: "references".to_string(),
      message: "参照ID（テーブルキー）に空文字列は使用できません".to_string(),
    });
  }
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use super::{
    DateCirca, DatePart, DateSeason, Name, NameError, NumberOrString, RawName, ReadReferencesError, References,
    ValidationError, parse_references, read_references, resolve,
  };

  /// `resolve` を実行し、集約された [`ValidationError`] のリストを取り出す。
  /// `resolve` が成功した場合はテスト失敗とする。
  fn resolve_errors(pre: References<RawName>) -> Vec<ValidationError> {
    let Err(ReadReferencesError::MultipleValidationErrors { errors }) = resolve(pre) else {
      panic!("expected MultipleValidationErrors");
    };
    return errors;
  }

  /// `parse_references` 用のダミーパス。
  fn dummy_source() -> &'static Path { return Path::new("test.toml"); }

  /// `style_path` を含む TOML 文字列を文字列のみで構築する（ファイル不要）。
  fn toml_with_style_path(style_path: &str, body: &str) -> String {
    return format!("style_path = \"{style_path}\"\n\n{body}");
  }

  #[test]
  fn read_references_returns_empty_when_path_is_none() {
    // Arrange / Act
    let result: References = read_references::<&Path>(None).unwrap();

    // Assert
    assert!(result.references.is_empty());
    assert_eq!(result.style_path, PathBuf::new());
  }

  #[test]
  fn parse_references_fails_on_invalid_toml_syntax() {
    // Arrange / Act
    let result = parse_references("style_path = \nthis is not valid toml", dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn resolve_fails_on_empty_id() {
    // Arrange
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[references.\"\"]\n\
       type = \"book\"\n\
       [[references.\"\".author]]\n\
       family = \"Doe\"\n",
    );
    let pre = parse_references(&toml, dummy_source()).unwrap();

    // Act
    let errors = resolve_errors(pre);

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { message, .. } if message.contains("空文字列")
    )));
  }

  #[test]
  fn parse_references_fails_on_duplicate_toml_keys() {
    // Arrange: keyed-table 形式では重複 ID は同一テーブルの再定義となり TOML パースエラーになる
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[references.dup]\n\
       type = \"book\"\n\
       [[references.dup.author]]\n\
       family = \"Doe\"\n\n\
       [references.dup]\n\
       type = \"book\"\n\
       [[references.dup.author]]\n\
       family = \"Roe\"\n",
    );

    // Act
    let result = parse_references(&toml, dummy_source());

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::ParseToml { .. })));
  }

  #[test]
  fn name_try_from_personal_drops_organization_only_fields() {
    // Arrange: family のみ → 個人著者
    let raw = RawName {
      family: Some("Doe".to_string()),
      given: Some("John".to_string()),
      suffix: Some("Jr.".to_string()),
      ..RawName::default()
    };

    // Act
    let name = Name::try_from(raw).unwrap();

    // Assert
    assert_eq!(
      name,
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
  fn name_try_from_organization() {
    // Arrange: literal のみ → 組織著者
    let raw = RawName {
      literal: Some("ACME Corp".to_string()),
      ..RawName::default()
    };

    // Act
    let name = Name::try_from(raw).unwrap();

    // Assert
    assert_eq!(
      name,
      Name::Organization {
        literal: "ACME Corp".to_string(),
      }
    );
  }

  #[test]
  fn name_try_from_fails_when_both_family_and_literal() {
    // Arrange
    let raw = RawName {
      family: Some("Doe".to_string()),
      literal: Some("ACME Corp".to_string()),
      ..RawName::default()
    };

    // Act / Assert
    assert!(matches!(Name::try_from(raw), Err(NameError::BothFamilyAndLiteral)));
  }

  #[test]
  fn name_try_from_fails_when_neither_family_nor_literal() {
    // Arrange: given のみ（family も literal もない）
    let raw = RawName {
      given: Some("John".to_string()),
      ..RawName::default()
    };

    // Act / Assert
    assert!(matches!(Name::try_from(raw), Err(NameError::NeitherFamilyNorLiteral)));
  }

  #[test]
  fn resolve_aggregates_name_error_with_field_path() {
    // Arrange: ref1.author[0] に family/literal 同時指定
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[references.ref1]\n\
       type = \"book\"\n\
       [[references.ref1.author]]\n\
       family = \"Doe\"\n\
       literal = \"ACME Corp\"\n",
    );
    let pre = parse_references(&toml, dummy_source()).unwrap();

    // Act
    let errors = resolve_errors(pre);

    // Assert: パスに著者フィールドの位置が含まれる
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, message } if path == "references.ref1.author[0]" && message.contains("両方")
    )));
  }

  #[test]
  fn resolve_aggregates_value_and_path_errors() {
    // Arrange: 不正な style_path、空 id、Name の不正を同時に発生させる
    let pre = parse_references(
      "style_path = \"/nonexistent/style.csl\"\n\n\
       [references.\"\"]\n\
       type = \"book\"\n\
       [[references.\"\".author]]\n\
       family = \"Doe\"\n\
       literal = \"ACME\"\n",
      dummy_source(),
    )
    .unwrap();

    // Act
    let result = resolve(pre);

    // Assert
    let Err(ReadReferencesError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().any(|e| matches!(e, ValidationError::StylePathResolution { .. })));
    assert!(
      errors
        .iter()
        .any(|e| matches!(e, ValidationError::Field { message, .. } if message.contains("空文字列")))
    );
    assert!(
      errors
        .iter()
        .any(|e| matches!(e, ValidationError::Field { message, .. } if message.contains("両方")))
    );
  }

  #[test]
  fn resolve_fails_when_style_path_does_not_exist() {
    // Arrange
    let pre = parse_references("style_path = \"/nonexistent/path/to/style.csl\"\n", dummy_source()).unwrap();

    // Act
    let result = resolve(pre);

    // Assert
    let Err(ReadReferencesError::MultipleValidationErrors { errors }) = result else {
      panic!("expected MultipleValidationErrors, got {result:?}");
    };
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::StylePathResolution { path, .. } if path == "/nonexistent/path/to/style.csl"
    )));
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
    let style_path = tempdir.path().join("style.csl");
    std::fs::write(&style_path, b"").unwrap();
    let references_path = tempdir.path().join("references.toml");
    std::fs::write(
      &references_path,
      toml_with_style_path(
        style_path.to_str().unwrap(),
        "[references.ref1]\n\
         type = \"book\"\n\
         title = \"Sample Book\"\n\
         [[references.ref1.author]]\n\
         family = \"Doe\"\n\
         given = \"John\"\n",
      ),
    )
    .unwrap();

    // Act
    let result = read_references(Some(&references_path)).unwrap();

    // Assert
    assert_eq!(result.references.len(), 1);
    assert!(result.references.contains_key("ref1"));
    assert_eq!(result.style_path, style_path.canonicalize().unwrap());
  }

  /// JSON 用のダミーパス。
  fn dummy_json_source() -> &'static Path { return Path::new("test.json"); }

  /// `style_path` と `references` を含む JSON 文字列を組み立てる。
  fn json_with_style_path(style_path: &str, references_json: &str) -> String {
    return format!("{{\"style_path\": \"{style_path}\", \"references\": {references_json}}}");
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
  fn resolve_fails_on_empty_id_for_json() {
    // Arrange
    let json =
      json_with_style_path("/tmp/style.csl", "{\"\": {\"type\": \"book\", \"author\": [{\"family\": \"Doe\"}]}}");
    let pre = parse_references(&json, dummy_json_source()).unwrap();

    // Act
    let errors = resolve_errors(pre);

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { message, .. } if message.contains("空文字列")
    )));
  }

  #[test]
  fn read_references_succeeds_with_valid_json_file() {
    // Arrange
    let tempdir = tempfile::tempdir().unwrap();
    let style_path = tempdir.path().join("style.csl");
    std::fs::write(&style_path, b"").unwrap();
    let references_path = tempdir.path().join("references.json");
    let json = json_with_style_path(
      style_path.to_str().unwrap(),
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
    assert_eq!(result.references.len(), 1);
    let reference = result.references.get("ref1").unwrap();
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
    assert_eq!(result.style_path, style_path.canonicalize().unwrap());
  }

  #[test]
  fn read_references_parses_structured_date_in_toml() {
    // Arrange
    let tempdir = tempfile::tempdir().unwrap();
    let style_path = tempdir.path().join("style.csl");
    std::fs::write(&style_path, b"").unwrap();
    let references_path = tempdir.path().join("references.toml");
    std::fs::write(
      &references_path,
      toml_with_style_path(
        style_path.to_str().unwrap(),
        "[references.ref1]\n\
         type = \"book\"\n\
         [[references.ref1.author]]\n\
         family = \"Doe\"\n\n\
         [references.ref1.issued]\n\
         date-parts = [[2024, 1, 15], [2024, 12, 31]]\n\
         circa = true\n\
         season = \"spring\"\n",
      ),
    )
    .unwrap();

    // Act
    let result = read_references(Some(&references_path)).unwrap();

    // Assert
    let reference = result.references.get("ref1").unwrap();
    let issued = reference.issued.as_ref().unwrap();
    let parts = issued.date_parts.as_ref().unwrap();
    assert_eq!(parts.len(), 2);
    assert!(matches!(
      parts[0].as_slice(),
      [
        DatePart::Number(2024),
        DatePart::Number(1),
        DatePart::Number(15)
      ]
    ));
    assert!(matches!(
      parts[1].as_slice(),
      [
        DatePart::Number(2024),
        DatePart::Number(12),
        DatePart::Number(31)
      ]
    ));
    assert!(matches!(issued.circa, Some(DateCirca::Bool(true))));
    assert!(matches!(&issued.season, Some(DateSeason::String(s)) if s == "spring"));
  }

  #[test]
  fn read_references_parses_structured_date_in_json() {
    // Arrange
    let tempdir = tempfile::tempdir().unwrap();
    let style_path = tempdir.path().join("style.csl");
    std::fs::write(&style_path, b"").unwrap();
    let references_path = tempdir.path().join("references.json");
    let json = json_with_style_path(
      style_path.to_str().unwrap(),
      "{\"ref1\": {\
         \"type\": \"book\", \
         \"issued\": {\
           \"date-parts\": [[2024, 1, 15]], \
           \"season\": 1, \
           \"circa\": true, \
           \"literal\": \"early 2024\", \
           \"raw\": \"Jan 15, 2024\", \
           \"edtf\": \"2024-01-15\"\
         }, \
         \"author\": [{\"family\": \"Doe\"}]\
       }}",
    );
    std::fs::write(&references_path, json).unwrap();

    // Act
    let result = read_references(Some(&references_path)).unwrap();

    // Assert
    let reference = result.references.get("ref1").unwrap();
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
    assert_eq!(issued.edtf.as_deref(), Some("2024-01-15"));
  }

  #[test]
  fn read_references_fails_on_unsupported_extension_file() {
    // Arrange
    let tempdir = tempfile::tempdir().unwrap();
    let references_path = tempdir.path().join("references.yaml");
    std::fs::write(&references_path, b"style_path: /tmp/style.csl").unwrap();

    // Act
    let result = read_references(Some(&references_path));

    // Assert
    assert!(matches!(result, Err(ReadReferencesError::UnsupportedExtension { .. })));
  }

  #[test]
  fn read_references_accepts_number_variables_as_integers_and_strings_in_toml() {
    // Arrange
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[references.ref1]\n\
       type = \"book\"\n\
       volume = 3\n\
       edition = 2.5\n\
       page = \"1-10\"\n\
       issue = 7\n\
       [[references.ref1.author]]\n\
       family = \"Doe\"\n",
    );

    // Act
    let pre = parse_references(&toml, dummy_source()).unwrap();
    let reference = pre.references.get("ref1").unwrap();

    // Assert
    assert!(matches!(reference.volume, Some(NumberOrString::Integer(3))));
    assert!(matches!(reference.edition, Some(NumberOrString::Float(value)) if (value - 2.5).abs() < f64::EPSILON));
    assert!(matches!(&reference.page, Some(NumberOrString::String(value)) if value == "1-10"));
    assert!(matches!(reference.issue, Some(NumberOrString::Integer(7))));
  }

  #[test]
  fn read_references_accepts_number_variables_as_integers_and_strings_in_json() {
    // Arrange
    let json = json_with_style_path(
      "/tmp/style.csl",
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
    let pre = parse_references(&json, dummy_json_source()).unwrap();
    let reference = pre.references.get("ref1").unwrap();

    // Assert
    assert!(matches!(reference.volume, Some(NumberOrString::Integer(3))));
    assert!(matches!(reference.edition, Some(NumberOrString::Float(value)) if (value - 2.5).abs() < f64::EPSILON));
    assert!(matches!(&reference.page, Some(NumberOrString::String(value)) if value == "1-10"));
    assert!(matches!(&reference.issue, Some(NumberOrString::String(value)) if value == "S2"));
  }
}
