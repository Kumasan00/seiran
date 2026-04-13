//! 参照定義ファイルの読み込みモジュール
//!
//! TOML 形式の参照定義ファイルを読み込み、ラベルとターゲットのペアを返す。

use std::{
  collections::HashMap,
  path::{Path, PathBuf},
};

use miette::Diagnostic;
use serde::{Deserialize, Deserializer, de};
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
}

/// 参照定義ファイル全体を表す構造体
///
/// スタイル名と参照定義のマップを保持する。
#[derive(Debug)]
pub struct References {
  /// 参照スタイル名（例: "apa", "ieee"）
  pub style_path: PathBuf,
  /// 参照定義のマップ（id をキー、Reference を値とする）
  pub references: HashMap<String, Reference>,
}

/// `References` のデシリアライズ用中間構造体
///
/// TOML からフラットにデシリアライズした後、`References` に変換する。
#[derive(Deserialize)]
struct ReferencesRaw {
  /// 参照スタイル名（例: "apa", "ieee"）
  style_path: PathBuf,
  /// 参照定義のリスト
  #[serde(default)]
  references: Vec<Reference>,
}

impl<'de> Deserialize<'de> for References {
  /// Vec<Reference> から `HashMap<String, Reference>` に変換してデシリアライズする
  ///
  /// # Errors
  ///
  /// デシリアライズに失敗した場合、または参照IDが重複している場合
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let raw = ReferencesRaw::deserialize(deserializer)?;
    let mut references = HashMap::new();

    for reference in raw.references {
      let id = reference.id.clone();
      if references.insert(id.clone(), reference).is_some() {
        return Err(de::Error::custom(format!("重複する参照ID: {id}")));
      }
    }

    return Ok(References {
      style_path: raw.style_path,
      references,
    });
  }
}

/// 個々の参照定義を表す構造体
///
/// CSL (Citation Style Language) に基づく文献情報を保持する。
/// <https://docs.citationstyles.org/en/stable/specification.html#appendix-iv-variables>
#[derive(Debug, Deserialize)]
pub struct Reference {
  /// 参照の一意識別子（引用キー）
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
  #[serde(rename = "citation-title")]
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
  pub authors: Option<Vec<Name>>,
  /// 議長
  pub chair: Option<Vec<Name>>,
  /// コレクション編集者
  #[serde(rename = "collection-editor")]
  pub collection_editor: Option<Vec<Name>>,
  /// 編纂者
  pub compiler: Option<Vec<Name>>,
  /// 作曲者
  pub composer: Option<Vec<Name>>,
  /// 収録著者（収録誌・書籍の著者）
  #[serde(rename = "container-author")]
  pub container_author: Option<Vec<Name>>,
  /// 貢献者
  pub contributor: Option<Vec<Name>>,
  /// キュレーター
  pub curator: Option<Vec<Name>>,
  /// 監督
  pub director: Option<Vec<Name>>,
  /// 編集者
  pub editor: Option<Vec<Name>>,
  /// 編集主幹
  #[serde(rename = "editorial-director")]
  pub editorial_director: Option<Vec<Name>>,
  /// 編集翻訳者
  #[serde(rename = "editorial-translator")]
  pub editor_translator: Option<Vec<Name>>,
  /// エグゼクティブプロデューサー
  #[serde(rename = "executive-producer")]
  pub executive_producer: Option<Vec<Name>>,
  /// ゲスト
  pub guest: Option<Vec<Name>>,
  /// 司会者
  pub host: Option<Vec<Name>>,
  /// 挿絵画家
  pub illustrator: Option<Vec<Name>>,
  /// インタビュアー
  pub interviewer: Option<Vec<Name>>,
  /// ナレーター
  pub narrator: Option<Vec<Name>>,
  /// 主催者
  pub organizer: Option<Vec<Name>>,
  /// 原著者
  #[serde(rename = "original-author")]
  pub original_author: Option<Vec<Name>>,
  /// 演者
  pub performer: Option<Vec<Name>>,
  /// プロデューサー
  pub producer: Option<Vec<Name>>,
  /// 受取人
  pub recipient: Option<Vec<Name>>,
  /// レビュー対象の著者
  #[serde(rename = "reviewed-author")]
  pub reviewed_author: Option<Vec<Name>>,
  /// 脚本家
  #[serde(rename = "script-writer")]
  pub script_writer: Option<Vec<Name>>,
  /// シリーズ制作者
  #[serde(rename = "series-creator")]
  pub series_creator: Option<Vec<Name>>,
  /// 翻訳者
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

/// 著者情報を表す列挙型
///
/// デシリアライズ時に `family` と `literal` の有無で個人著者か組織著者かを判別する。
/// 両方が存在する場合はエラーとなる。
#[derive(Debug)]
pub enum Name {
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

/// `Name` のデシリアライズ用中間構造体
///
/// TOML からフラットにデシリアライズした後、`Name` 列挙型に変換する。
#[derive(Deserialize)]
struct NameRaw {
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

impl<'de> Deserialize<'de> for Name {
  /// `family` と `literal` の組み合わせに基づいて `Name` をデシリアライズする
  ///
  /// - `family` のみ → `Person`
  /// - `literal` のみ → `Organization`
  /// - 両方あり → エラー
  /// - 両方なし → エラー
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let raw = NameRaw::deserialize(deserializer)?;

    match (raw.family, raw.literal) {
      (Some(_), Some(_)) => {
        return Err(de::Error::custom(
          "著者に `family` と `literal` の両方を指定することはできません。個人著者には `family` を、組織著者には `literal` を使用してください",
        ));
      },
      (None, Some(literal)) => {
        return Ok(Name::Organization { literal });
      },
      (Some(family), None) => {
        return Ok(Name::Person {
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
/// * `path` - 参照定義 TOML ファイルのパス。`None` の場合は空の参照定義を返す。
///
/// # Returns
///
/// 参照定義の構造体を返す。参照定義ファイルが指定されていない場合は、空の `HashMap` を持つ構造体を返す。
///
/// # Errors
///
/// - ファイルの読み込みに失敗した場合
/// - TOML のパースに失敗した場合
pub fn read_references<P: AsRef<Path>>(path: Option<P>) -> Result<References, ReadReferencesError> {
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
    let count = references.references.len();
    info!(count, "参照定義ファイルの読み込みが完了しました");
    return Ok(references);
  } else {
    info!("参照定義ファイルが指定されていないため、空の参照定義を返します");
    return Ok(References {
      style_path: PathBuf::new(),
      references: HashMap::new(),
    });
  }
}
