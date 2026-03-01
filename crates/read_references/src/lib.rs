//! 参照定義ファイルの読み込みモジュール
//!
//! TOML 形式の参照定義ファイルを読み込み、ラベルとターゲットのペアを返す。

#![allow(unused_assignments)]

use std::path::Path;

use miette::Diagnostic;
use serde::{Deserialize, Deserializer, de};
use thiserror::Error;
use toml::value::Datetime;
use tracing::info;

/// 参照定義ファイル読み込み時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum ReadReferencesError {
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
}

/// 参照定義ファイル全体を表す構造体
///
/// スタイル名と参照定義のリストを保持する。
#[derive(Debug, Deserialize)]
pub struct References {
  /// 参照スタイル名（例: "apa", "ieee"）
  pub style: String,
  /// 参照定義のリスト（省略可能）
  pub references: Option<Vec<Reference>>,
}

/// 個々の参照定義を表す構造体
///
/// CSL (Citation Style Language) に基づく文献情報を保持する。
#[derive(Debug, Deserialize)]
pub struct Reference {
  /// 参照の一意識別子（引用キー）
  pub id: String,
  /// 参照の種類（書籍、論文など）
  #[serde(rename = "type")]
  pub reference_type: ReferenceType,
  /// タイトル
  pub title: Option<String>,
  /// 著者リスト
  pub authors: Option<Vec<Author>>,
  /// 発行日
  pub issued: Option<Datetime>,
  /// 収録誌・書名（例: ジャーナル名、書籍シリーズ名）
  #[serde(rename = "container-title")]
  pub container_title: Option<String>,
  /// 巻号（ボリューム）
  pub volume: Option<String>,
  /// 号数（イシュー）
  pub issue: Option<String>,
  /// ページ範囲（例: "1-10"）
  pub page: Option<String>,
  /// 出版社名
  pub publisher: Option<String>,
  /// 出版地
  #[serde(rename = "publisher-place")]
  pub publisher_place: Option<String>,
  /// DOI (Digital Object Identifier)
  #[serde(rename = "DOI")]
  pub doi: Option<String>,
  /// URL
  #[serde(rename = "URL")]
  pub url: Option<String>,
  /// ISBN (International Standard Book Number)
  #[serde(rename = "ISBN")]
  pub isbn: Option<String>,
  /// 補足情報
  pub note: Option<String>,
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

/// 著者情報を表す列挙型
///
/// デシリアライズ時に `family` と `literal` の有無で個人著者か組織著者かを判別する。
/// 両方が存在する場合はエラーとなる。
#[derive(Debug)]
pub enum Author {
  /// 組織著者
  Organization {
    /// 組織名・リテラル表記
    literal: String,
  },
  /// 個人著者
  Person {
    /// 姓
    family: String,
    /// 名
    given: Option<String>,
    /// dropping particle（例: "de" in "de Gaulle"）
    dropping_particle: Option<String>,
    /// non-dropping particle（例: "van" in "van Beethoven"）
    non_dropping_particle: Option<String>,
    /// 接尾辞（例: "Jr.", "III"）
    suffix: Option<String>,
  },
}

/// `Author` のデシリアライズ用中間構造体
///
/// TOML からフラットにデシリアライズした後、`Author` 列挙型に変換する。
#[derive(Deserialize)]
struct AuthorRaw {
  /// 組織名・リテラル表記（組織著者の場合）
  literal: Option<String>,
  /// 姓（個人著者の場合）
  family: Option<String>,
  /// 名（個人著者の場合）
  given: Option<String>,
  /// dropping particle（例: "de" in "de Gaulle"）
  #[serde(rename = "dropping-particle")]
  dropping_particle: Option<String>,
  /// non-dropping particle（例: "van" in "van Beethoven"）
  #[serde(rename = "non-dropping-particle")]
  non_dropping_particle: Option<String>,
  /// 接尾辞（例: "Jr.", "III"）
  suffix: Option<String>,
}

impl<'de> Deserialize<'de> for Author {
  /// `family` と `literal` の組み合わせに基づいて `Author` をデシリアライズする
  ///
  /// - `family` のみ → `Person`
  /// - `literal` のみ → `Organization`
  /// - 両方あり → エラー
  /// - 両方なし → エラー
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let raw = AuthorRaw::deserialize(deserializer)?;

    match (raw.family, raw.literal) {
      (Some(_), Some(_)) => {
        return Err(de::Error::custom(
          "著者に `family` と `literal` の両方を指定することはできません。個人著者には `family` を、組織著者には `literal` を使用してください",
        ));
      },
      (None, Some(literal)) => {
        return Ok(Author::Organization { literal });
      },
      (Some(family), None) => {
        return Ok(Author::Person {
          family,
          given: raw.given,
          dropping_particle: raw.dropping_particle,
          non_dropping_particle: raw.non_dropping_particle,
          suffix: raw.suffix,
        });
      },
      (None, None) => {
        return Err(de::Error::custom("著者には `family`（個人著者）または `literal`（組織著者）のいずれかが必要です"));
      },
    }
  }
}

/// 参照定義ファイルを読み込む
///
/// # Arguments
///
/// * `path` - 参照定義 TOML ファイルのパス。`None` の場合は参照定義なしとみなす。
///
/// # Returns
///
/// ファイルが指定された場合は `Some(Vec<Reference>)`、指定されなかった場合は `None` を返す。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - TOML のパースに失敗した場合
pub fn read_references<P: AsRef<Path>>(path: Option<P>) -> miette::Result<Option<References>> {
  #[allow(clippy::redundant_else)]
  if let Some(path) = path {
    let path_ref = path.as_ref();
    info!(references_path = %path_ref.display(), "参照定義ファイルの読み込みを開始します");
    let file = std::fs::read_to_string(path_ref).map_err(|source| ReadReferencesError::ReadFile {
      path: path_ref.display().to_string(),
      source,
    })?;
    let references: References = toml::from_str(&file).map_err(|source| ReadReferencesError::ParseToml {
      path: path_ref.display().to_string(),
      source,
    })?;
    let count = references.references.as_ref().map_or(0, std::vec::Vec::len);
    info!(count, "参照定義ファイルの読み込みが完了しました");
    return Ok(Some(references));
  } else {
    info!("参照定義ファイルが指定されていないため、スキップします");
    return Ok(None);
  }
}
