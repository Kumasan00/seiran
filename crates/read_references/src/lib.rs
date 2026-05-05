//! 参照定義ファイルの読み込みモジュール
//!
//! TOML 形式の参照定義ファイルを読み込み、`garde` による宣言的バリデーションを経て
//! `id` をキーとする参照定義のマップを返す。フィールド単位の不正・著者の判別不能
//! （`family` と `literal` の同時指定／不指定）・参照 ID の重複は、すべて
//! `MultipleValidationErrors` に集約して 1 度に報告する。

use std::{
  collections::{HashMap, HashSet},
  path::{Path, PathBuf},
};

use garde::Validate;
use miette::Diagnostic;
use serde::Deserialize;
use thiserror::Error;
use toml::value::Datetime;
use tracing::info;

/// 参照定義ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ReadReferencesError {
  /// 参照定義ファイルの読み込みに失敗した場合
  #[error("参照定義ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(references::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// TOML 解析に失敗した場合
  #[error("参照定義ファイルの TOML 解析に失敗しました: {path}")]
  #[diagnostic(code(references::parse_toml), help("TOML の構文を確認してください。"))]
  ParseToml {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: toml::de::Error,
  },
  /// 複合バリデーションエラー（複数のエラーをまとめて報告）
  #[error("参照定義のバリデーションに失敗しました。")]
  #[diagnostic(code(references::multiple_validation_errors))]
  MultipleValidationErrors {
    /// 検証で検出されたすべてのエラー
    #[related]
    errors: Vec<ValidationError>,
  },
}

/// 参照定義値バリデーションのエラー詳細。
#[derive(Debug, Error, Diagnostic)]
pub enum ValidationError {
  /// garde が検出した参照定義値の不正、または重複 ID などの構造的不正
  #[error("'{path}': {message}")]
  #[diagnostic(
    code(references::validation::field),
    help("references.toml の該当フィールドの値を確認してください。")
  )]
  Field {
    /// 不正なフィールドのパス（例: `references[0].id`）
    path: String,
    /// 不正の内容
    message: String,
  },
  /// CSL スタイルファイルのパスを解決できない
  #[error("CSL スタイルファイルのパスを解決できませんでした: {path}")]
  #[diagnostic(
    code(references::validation::style_path),
    help("`style_path` で指定されたファイルが存在し、読み取り権限があることを確認してください。")
  )]
  StylePathResolution {
    /// 解決できなかったパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// 参照定義ファイル全体を表す構造体
///
/// 検証通過後の `id` をキーとする参照定義のマップを保持する。
#[derive(Debug)]
pub struct References {
  /// 参照スタイル（CSL）ファイルへのパス
  pub style_path: PathBuf,
  /// 参照定義のマップ（id をキー、`Reference` を値とする）
  pub references: HashMap<String, Reference>,
}

/// `References` のデシリアライズ・検証用中間構造体
///
/// TOML からフラットにデシリアライズした後、`garde::Validate` による検証と
/// 重複 ID チェックを経て `References` に変換する。
#[derive(Debug, Deserialize, Validate)]
#[garde(allow_unvalidated)]
pub struct PreReferences {
  /// 参照スタイル（CSL）ファイルへのパス
  pub style_path: PathBuf,
  /// 参照定義のリスト（重複 ID は後段で検出する）
  #[serde(default)]
  #[garde(dive)]
  pub references: Vec<Reference>,
}

/// 個々の参照定義を表す構造体
///
/// CSL (Citation Style Language) に基づく文献情報を保持する。
/// <https://docs.citationstyles.org/en/stable/specification.html#appendix-iv-variables>
#[derive(Debug, Deserialize, Validate)]
#[garde(allow_unvalidated)]
pub struct Reference {
  /// 参照の一意識別子（引用キー、空文字列不可）
  #[garde(length(min = 1))]
  pub id: String,
  /// 参照の種類（書籍、論文など）
  #[serde(rename = "type")]
  pub reference_type: ReferenceType,

  // Standard Variables
  /// 要旨・抄録
  #[serde(rename = "abstract")]
  pub item_abstract: Option<String>,
  /// 注釈
  pub annote: Option<String>,
  /// アーカイブ名
  pub archive: Option<String>,
  /// アーカイブ内コレクション名
  pub archive_collection: Option<String>,
  /// アーカイブ内の場所
  pub archive_location: Option<String>,
  /// アーカイブの所在地
  #[serde(rename = "archive-place")]
  pub archive_place: Option<String>,
  /// 発行機関
  pub authority: Option<String>,
  /// 図書館の請求記号
  #[serde(rename = "call-number")]
  pub call_number: Option<String>,
  /// 引用キー（BibTeX 互換）
  #[serde(rename = "citation-key")]
  pub citation_key: Option<String>,
  /// 引用ラベル
  #[serde(rename = "citation-label")]
  pub citation_label: Option<String>,
  /// コレクションのタイトル
  #[serde(rename = "collection-title")]
  pub collection_title: Option<String>,
  /// 収録誌・書名（例: ジャーナル名、書籍シリーズ名）
  #[serde(rename = "container-title")]
  pub container_title: Option<String>,
  /// 収録誌・書名（短縮形）
  #[serde(rename = "container-title-short")]
  pub container_title_short: Option<String>,
  /// 物理的寸法
  pub dimensions: Option<String>,
  /// 部門・部局名
  pub division: Option<String>,
  /// DOI (Digital Object Identifier)
  #[serde(rename = "DOI")]
  pub doi: Option<String>,
  /// イベント名（非推奨: event-title を使用）
  pub event: Option<String>,
  /// イベントのタイトル（例: 会議名）
  #[serde(rename = "event-title")]
  pub event_title: Option<String>,
  /// イベント開催地
  #[serde(rename = "event-place")]
  pub event_place: Option<String>,
  /// ジャンル・種別（例: 技術報告書、修士論文）
  pub genre: Option<String>,
  /// ISBN (International Standard Book Number)
  #[serde(rename = "ISBN")]
  pub isbn: Option<String>,
  /// ISSN (International Standard Serial Number)
  #[serde(rename = "ISSN")]
  pub issn: Option<String>,
  /// 管轄区域（法律文書など）
  pub jurisdiction: Option<String>,
  /// キーワード
  pub keyword: Option<String>,
  /// 言語
  pub language: Option<String>,
  /// ライセンス
  pub license: Option<String>,
  /// 媒体（例: CD-ROM、DVD）
  pub medium: Option<String>,
  /// 補足情報
  pub note: Option<String>,
  /// 原著の出版社
  #[serde(rename = "original-publisher")]
  pub original_publisher: Option<String>,
  /// 原著の出版地
  #[serde(rename = "original-publisher-place")]
  pub original_publisher_place: Option<String>,
  /// 原題
  #[serde(rename = "original-title")]
  pub original_title: Option<String>,
  /// パートのタイトル
  #[serde(rename = "part-title")]
  pub part_title: Option<String>,
  /// `PubMed Central ID`
  #[serde(rename = "PMCID")]
  pub pmcid: Option<String>,
  /// `PubMed ID`
  #[serde(rename = "PMID")]
  pub pmid: Option<String>,
  /// 出版社名
  pub publisher: Option<String>,
  /// 出版地
  #[serde(rename = "publisher-place")]
  pub publisher_place: Option<String>,
  /// 参考文献リスト
  pub references: Option<String>,
  /// レビュー対象のジャンル
  #[serde(rename = "reviewed-genre")]
  pub reviewed_genre: Option<String>,
  /// レビュー対象のタイトル
  #[serde(rename = "reviewed-title")]
  pub reviewed_title: Option<String>,
  /// 地図の縮尺
  pub scale: Option<String>,
  /// 情報源
  pub source: Option<String>,
  /// 出版状態（例: in press, forthcoming）
  pub status: Option<String>,
  /// タイトル
  pub title: Option<String>,
  /// タイトル（短縮形）
  #[serde(rename = "title-short")]
  pub title_short: Option<String>,
  /// URL
  #[serde(rename = "URL")]
  pub url: Option<String>,
  /// 巻のタイトル
  #[serde(rename = "volume-title")]
  pub volume_title: Option<String>,
  /// 年サフィックス（同一著者・同一年の文献を区別）
  #[serde(rename = "year-suffix")]
  pub year_suffix: Option<String>,

  // Number Variables
  /// 章番号
  #[serde(rename = "chapter-number")]
  pub chapter_number: Option<String>,
  /// 引用番号
  #[serde(rename = "citation-number")]
  pub citation_number: Option<String>,
  /// コレクション番号
  #[serde(rename = "collection-number")]
  pub collection_number: Option<String>,
  /// 版（例: 第2版）
  pub edition: Option<String>,
  /// 最初の参照脚注番号
  #[serde(rename = "first-reference-note-number")]
  pub first_reference_note_number: Option<String>,
  /// 号数（イシュー）
  pub issue: Option<String>,
  /// 引用箇所（ページ番号など）
  pub locator: Option<String>,
  /// 番号（汎用）
  pub number: Option<String>,
  /// 総ページ数
  #[serde(rename = "number-of-pages")]
  pub number_of_pages: Option<String>,
  /// 総巻数
  #[serde(rename = "number-of-volumes")]
  pub number_of_volumes: Option<String>,
  /// ページ範囲（例: "1-10"）
  pub page: Option<String>,
  /// 開始ページ番号
  #[serde(rename = "page-first")]
  pub page_first: Option<String>,
  /// パート番号
  #[serde(rename = "part-number")]
  pub part_number: Option<String>,
  /// 刷番号
  #[serde(rename = "printing-number")]
  pub printing_number: Option<String>,
  /// セクション
  pub section: Option<String>,
  /// 補遺番号
  #[serde(rename = "supplement-number")]
  pub supplement_number: Option<String>,
  /// バージョン
  pub version: Option<String>,
  /// 巻号（ボリューム）
  pub volume: Option<String>,

  // Date Variables
  /// アクセス日
  pub accessed: Option<Datetime>,
  /// 利用可能日
  #[serde(rename = "available-date")]
  pub available_date: Option<Datetime>,
  /// イベント開催日
  #[serde(rename = "event-date")]
  pub event_date: Option<Datetime>,
  /// 発行日
  pub issued: Option<Datetime>,
  /// 原著の発行日
  #[serde(rename = "original-date")]
  pub original_date: Option<Datetime>,
  /// 提出日
  pub submitted: Option<Datetime>,

  // Name Variables
  /// 著者リスト
  #[garde(dive)]
  pub author: Option<Vec<Name>>,
  /// 議長
  #[garde(dive)]
  pub chair: Option<Vec<Name>>,
  /// コレクション編集者
  #[serde(rename = "collection-editor")]
  #[garde(dive)]
  pub collection_editor: Option<Vec<Name>>,
  /// 編纂者
  #[garde(dive)]
  pub compiler: Option<Vec<Name>>,
  /// 作曲者
  #[garde(dive)]
  pub composer: Option<Vec<Name>>,
  /// 収録著者（収録誌・書籍の著者）
  #[serde(rename = "container-author")]
  #[garde(dive)]
  pub container_author: Option<Vec<Name>>,
  /// 貢献者
  #[garde(dive)]
  pub contributor: Option<Vec<Name>>,
  /// キュレーター
  #[garde(dive)]
  pub curator: Option<Vec<Name>>,
  /// 監督
  #[garde(dive)]
  pub director: Option<Vec<Name>>,
  /// 編集者
  #[garde(dive)]
  pub editor: Option<Vec<Name>>,
  /// 編集主幹
  #[serde(rename = "editorial-director")]
  #[garde(dive)]
  pub editorial_director: Option<Vec<Name>>,
  /// 編集翻訳者
  #[serde(rename = "editor-translator")]
  #[garde(dive)]
  pub editor_translator: Option<Vec<Name>>,
  /// エグゼクティブプロデューサー
  #[serde(rename = "executive-producer")]
  #[garde(dive)]
  pub executive_producer: Option<Vec<Name>>,
  /// ゲスト
  #[garde(dive)]
  pub guest: Option<Vec<Name>>,
  /// 司会者
  #[garde(dive)]
  pub host: Option<Vec<Name>>,
  /// 挿絵画家
  #[garde(dive)]
  pub illustrator: Option<Vec<Name>>,
  /// インタビュアー
  #[garde(dive)]
  pub interviewer: Option<Vec<Name>>,
  /// ナレーター
  #[garde(dive)]
  pub narrator: Option<Vec<Name>>,
  /// 主催者
  #[garde(dive)]
  pub organizer: Option<Vec<Name>>,
  /// 原著者
  #[serde(rename = "original-author")]
  #[garde(dive)]
  pub original_author: Option<Vec<Name>>,
  /// 演者
  #[garde(dive)]
  pub performer: Option<Vec<Name>>,
  /// プロデューサー
  #[garde(dive)]
  pub producer: Option<Vec<Name>>,
  /// 受取人
  #[garde(dive)]
  pub recipient: Option<Vec<Name>>,
  /// レビュー対象の著者
  #[serde(rename = "reviewed-author")]
  #[garde(dive)]
  pub reviewed_author: Option<Vec<Name>>,
  /// 脚本家
  #[serde(rename = "script-writer")]
  #[garde(dive)]
  pub script_writer: Option<Vec<Name>>,
  /// シリーズ制作者
  #[serde(rename = "series-creator")]
  #[garde(dive)]
  pub series_creator: Option<Vec<Name>>,
  /// 翻訳者
  #[garde(dive)]
  pub translator: Option<Vec<Name>>,
}

/// 参照の種類を表す列挙型
///
/// CSL (Citation Style Language) で定義されている文献タイプに対応する。
#[derive(Debug, Deserialize)]
pub enum ReferenceType {
  #[serde(rename = "article")]
  Article,
  #[serde(rename = "article-journal")]
  ArticleJournal,
  #[serde(rename = "article-magazine")]
  ArticleMagazine,
  #[serde(rename = "article-newspaper")]
  ArticleNewspaper,
  #[serde(rename = "bill")]
  Bill,
  #[serde(rename = "book")]
  Book,
  #[serde(rename = "broadcast")]
  Broadcast,
  #[serde(rename = "chapter")]
  Chapter,
  #[serde(rename = "classic")]
  Classic,
  #[serde(rename = "collection")]
  Collection,
  #[serde(rename = "dataset")]
  Dataset,
  #[serde(rename = "document")]
  Document,
  #[serde(rename = "entry")]
  Entry,
  #[serde(rename = "entry-dictionary")]
  EntryDictionary,
  #[serde(rename = "entry-encyclopedia")]
  EntryEncyclopedia,
  #[serde(rename = "event")]
  Event,
  #[serde(rename = "figure")]
  Figure,
  #[serde(rename = "graphic")]
  Graphic,
  #[serde(rename = "hearing")]
  Hearing,
  #[serde(rename = "interview")]
  Interview,
  #[serde(rename = "legal_case")]
  LegalCase,
  #[serde(rename = "legislation")]
  Legislation,
  #[serde(rename = "manuscript")]
  Manuscript,
  #[serde(rename = "map")]
  Map,
  #[serde(rename = "motion_picture")]
  MotionPicture,
  #[serde(rename = "musical_score")]
  MusicalScore,
  #[serde(rename = "pamphlet")]
  Pamphlet,
  #[serde(rename = "paper-conference")]
  PaperConference,
  #[serde(rename = "patent")]
  Patent,
  #[serde(rename = "performance")]
  Performance,
  #[serde(rename = "periodical")]
  Periodical,
  #[serde(rename = "personal_communication")]
  PersonalCommunication,
  #[serde(rename = "post")]
  Post,
  #[serde(rename = "post-weblog")]
  PostWeblog,
  #[serde(rename = "regulation")]
  Regulation,
  #[serde(rename = "report")]
  Report,
  #[serde(rename = "review")]
  Review,
  #[serde(rename = "review-book")]
  ReviewBook,
  #[serde(rename = "software")]
  Software,
  #[serde(rename = "song")]
  Song,
  #[serde(rename = "speech")]
  Speech,
  #[serde(rename = "standard")]
  Standard,
  #[serde(rename = "thesis")]
  Thesis,
  #[serde(rename = "treaty")]
  Treaty,
  #[serde(rename = "webpage")]
  Webpage,
}

/// 著者情報を表す構造体
///
/// `garde` の検証により、`family`（個人著者）と `literal`（組織著者）の
/// いずれか一方のみが指定されていることが保証される。
/// 両方が指定されている、または両方とも指定されていない場合は検証エラーとなる
/// （`Validate` は手動実装：`#[garde(custom)]` の struct-level 構文が
/// garde 0.22 では未対応のため）。
#[derive(Debug, Deserialize)]
pub struct Name {
  /// 組織名・リテラル表記（組織著者の場合）
  pub literal: Option<String>,
  /// 姓（個人著者の場合）
  pub family: Option<String>,
  /// 名（個人著者の場合）
  pub given: Option<String>,
  /// dropping particle（例: "de" in "de Gaulle"）
  #[serde(rename = "dropping-particle")]
  pub dropping_particle: Option<String>,
  /// non-dropping particle（例: "van" in "van Beethoven"）
  #[serde(rename = "non-dropping-particle")]
  pub non_dropping_particle: Option<String>,
  /// 接尾辞（例: "Jr.", "III"）
  pub suffix: Option<String>,
}

/// `Name` の `family` と `literal` の排他性を検証する手動 `Validate` 実装。
///
/// - `family` のみ → 個人著者として有効
/// - `literal` のみ → 組織著者として有効
/// - 両方あり、両方なし → エラー
///
/// `Reference` の各著者フィールドが `#[garde(dive)]` でこの実装を呼び出すため、
/// 違反は `pre.validate()` の戻り値 `Report` にそのまま集約される。
impl Validate for Name {
  type Context = ();

  fn validate_into(&self, _ctx: &Self::Context, parent: &mut dyn FnMut() -> garde::Path, report: &mut garde::Report) {
    match (&self.family, &self.literal) {
      (Some(_), Some(_)) => {
        report.append(
          parent(),
          garde::Error::new(
            "`family` と `literal` の両方を指定することはできません。個人著者には `family` を、組織著者には `literal` を使用してください",
          ),
        );
      },
      (None, None) => {
        report
          .append(parent(), garde::Error::new("`family`（個人著者）または `literal`（組織著者）のいずれかが必要です"));
      },
      _ => {},
    }
  }
}

/// 参照定義ファイルを読み込む利便ラッパ
///
/// [`parse_references`] → [`resolve`] を連結します。生産コードはこの関数を使用します。
///
/// # Arguments
///
/// * `path` - 参照定義 TOML ファイルのパス。`None` の場合は空の参照定義を返す。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - TOML のパースに失敗した場合
/// - フィールド単位の検証や参照 ID の重複検出に失敗した場合
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
  let pre = parse_references(&content, path_ref)?;
  let references = resolve(pre)?;
  let count = references.references.len();
  info!(count, "参照定義ファイルの読み込みが完了しました");
  return Ok(references);
}

/// TOML テキストを [`PreReferences`] にパースします（I/O なし）。
///
/// `source_path` はエラー報告に使う表示用パスで、ファイルシステムへのアクセスには使われません。
/// 値検証は行いません。検証は [`validate_values`] または [`resolve`] で実行します。
///
/// # Errors
///
/// TOML の構文が不正な場合に [`ReadReferencesError::ParseToml`] を返します。
fn parse_references(text: &str, source_path: &Path) -> Result<PreReferences, ReadReferencesError> {
  return toml::from_str(text).map_err(|source| ReadReferencesError::ParseToml {
    path: source_path.display().to_string(),
    source,
  });
}

/// [`PreReferences`] からパス解決を行い [`References`] を構築します。
///
/// 値検証も内部で再実行し、値違反と `style_path` の解決エラーを 1 回の
/// [`ReadReferencesError::MultipleValidationErrors`] に集約します。
///
/// # Errors
///
/// 値検証または `style_path` の正規化に違反があった場合は
/// [`ReadReferencesError::MultipleValidationErrors`] を返します。
fn resolve(pre: PreReferences) -> Result<References, ReadReferencesError> {
  let mut errors: Vec<ValidationError> = match validate_values(&pre) {
    Ok(()) => Vec::new(),
    Err(value_errors) => value_errors,
  };

  let style_path = match pre.style_path.canonicalize() {
    Ok(canon) => canon,
    Err(source) => {
      errors.push(ValidationError::StylePathResolution {
        path: pre.style_path.display().to_string(),
        source,
      });
      return Err(ReadReferencesError::MultipleValidationErrors { errors });
    },
  };

  if !errors.is_empty() {
    return Err(ReadReferencesError::MultipleValidationErrors { errors });
  }

  let mut references = HashMap::with_capacity(pre.references.len());
  for reference in pre.references {
    references.insert(reference.id.clone(), reference);
  }
  return Ok(References {
    style_path,
    references,
  });
}

/// [`PreReferences`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証と参照 ID の重複チェックをまとめて返します。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(pre: &PreReferences) -> Result<(), Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();
  if let Err(report) = pre.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }
  validate_unique_ids(&pre.references, &mut errors);
  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
}

/// 参照 ID の一意性を検証し、違反を `errors` に追加する。
///
/// garde の field-level 検証では表現できない要素間の制約のため、`PreReferences::validate` の
/// 後に明示的に呼び出す。同一 ID の 2 件目以降の出現位置をすべて報告する。
fn validate_unique_ids(references: &[Reference], errors: &mut Vec<ValidationError>) {
  let mut seen: HashSet<&str> = HashSet::new();
  for (index, reference) in references.iter().enumerate() {
    if !seen.insert(reference.id.as_str()) {
      errors.push(ValidationError::Field {
        path: format!("references[{index}]"),
        message: format!("参照ID '{}' が重複しています", reference.id),
      });
    }
  }
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use super::{
    ReadReferencesError, References, ValidationError, parse_references, read_references, resolve, validate_values,
  };

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
  fn validate_values_fails_on_empty_id() {
    // Arrange
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[[references]]\n\
       id = \"\"\n\
       type = \"book\"\n\
       [[references.author]]\n\
       family = \"Doe\"\n",
    );
    let pre = parse_references(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { path, .. } if path.contains("id")
    )));
  }

  #[test]
  fn validate_values_fails_on_duplicate_ids() {
    // Arrange
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[[references]]\n\
       id = \"dup\"\n\
       type = \"book\"\n\
       [[references.author]]\n\
       family = \"Doe\"\n\n\
       [[references]]\n\
       id = \"dup\"\n\
       type = \"book\"\n\
       [[references.author]]\n\
       family = \"Roe\"\n",
    );
    let pre = parse_references(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { message, .. } if message.contains("重複")
    )));
  }

  #[test]
  fn validate_values_fails_when_name_has_both_family_and_literal() {
    // Arrange
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[[references]]\n\
       id = \"ref1\"\n\
       type = \"book\"\n\
       [[references.author]]\n\
       family = \"Doe\"\n\
       literal = \"ACME Corp\"\n",
    );
    let pre = parse_references(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { message, .. } if message.contains("両方")
    )));
  }

  #[test]
  fn validate_values_fails_when_name_has_neither_family_nor_literal() {
    // Arrange
    let toml = toml_with_style_path(
      "/tmp/style.csl",
      "[[references]]\n\
       id = \"ref1\"\n\
       type = \"book\"\n\
       [[references.author]]\n\
       given = \"John\"\n",
    );
    let pre = parse_references(&toml, dummy_source()).unwrap();

    // Act
    let errors = validate_values(&pre).unwrap_err();

    // Assert
    assert!(errors.iter().any(|error| matches!(
      error,
      ValidationError::Field { message, .. } if message.contains("いずれか")
    )));
  }

  #[test]
  fn resolve_aggregates_value_and_path_errors() {
    // Arrange: 不正な style_path、空 id、Name の不正を同時に発生させる
    let pre = parse_references(
      "style_path = \"/nonexistent/style.csl\"\n\n\
       [[references]]\n\
       id = \"\"\n\
       type = \"book\"\n\
       [[references.author]]\n\
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
    assert!(errors.iter().any(|e| matches!(e, ValidationError::Field { path, .. } if path.contains("id"))));
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
        "[[references]]\n\
         id = \"ref1\"\n\
         type = \"book\"\n\
         title = \"Sample Book\"\n\
         [[references.author]]\n\
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
}
