//! CSL (Citation Style Language) 文献情報のデータモデル。
//!
//! 参照定義ファイル全体を表す [`References`] と個々の文献を表す [`Reference`]、文献タイプの
//! [`ReferenceType`]、数値または文字列を許容する [`NumberOrString`] を定義する。著者名の表現は
//! 型引数 `N` で切り替える（serde 境界では [`RawName`](crate::RawName)、確定後は [`Name`]）。

use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;

use crate::{date::Date, name::Name};

/// 参照定義ファイル全体を表す構造体
///
/// 型引数 `N` は著者名の表現を切り替える。serde 境界では `References<RawName>` としてパースし、
/// [`resolve`](crate::resolve) で名前検証（family/literal 排他）・空 ID チェック・`style_path` の正規化を
/// 経て、確定済みの `References<Name>`（既定）へ変換する。`references` は keyed-table 形式
/// （テーブルキーが参照 ID）でデシリアライズするため、`id` をキーとするマップに直接展開される。
#[derive(Debug, Deserialize)]
pub struct References<N = Name> {
  /// 参照スタイル（CSL）ファイルへのパス（[`resolve`](crate::resolve) 通過後は正規化済み）
  pub style_path: PathBuf,
  /// 参照定義のマップ（id をキー、`Reference` を値とする）
  #[serde(default)]
  pub references: HashMap<String, Reference<N>>,
}

/// 個々の参照定義を表す構造体
///
/// CSL (Citation Style Language) に基づく文献情報を保持する。
/// 参照 ID は keyed-table 形式のテーブルキーとして保持されるため、本構造体には持たない。
/// 型引数 `N` は著者名の表現で、serde 境界では [`RawName`](crate::RawName)、確定後（既定）は [`Name`]
/// となる。
/// <https://docs.citationstyles.org/en/stable/specification.html#appendix-iv-variables>
#[derive(Debug, Deserialize)]
pub struct Reference<N = Name> {
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
  pub chapter_number: Option<NumberOrString>,
  /// 引用番号
  #[serde(rename = "citation-number")]
  pub citation_number: Option<NumberOrString>,
  /// コレクション番号
  #[serde(rename = "collection-number")]
  pub collection_number: Option<NumberOrString>,
  /// 版（例: 第2版）
  pub edition: Option<NumberOrString>,
  /// 最初の参照脚注番号
  #[serde(rename = "first-reference-note-number")]
  pub first_reference_note_number: Option<NumberOrString>,
  /// 号数（イシュー）
  pub issue: Option<NumberOrString>,
  /// 引用箇所（ページ番号など）
  pub locator: Option<NumberOrString>,
  /// 番号（汎用）
  pub number: Option<NumberOrString>,
  /// 総ページ数
  #[serde(rename = "number-of-pages")]
  pub number_of_pages: Option<NumberOrString>,
  /// 総巻数
  #[serde(rename = "number-of-volumes")]
  pub number_of_volumes: Option<NumberOrString>,
  /// ページ範囲（例: "1-10"）
  pub page: Option<NumberOrString>,
  /// 開始ページ番号
  #[serde(rename = "page-first")]
  pub page_first: Option<NumberOrString>,
  /// パート番号
  #[serde(rename = "part-number")]
  pub part_number: Option<NumberOrString>,
  /// 刷番号
  #[serde(rename = "printing-number")]
  pub printing_number: Option<NumberOrString>,
  /// セクション
  pub section: Option<NumberOrString>,
  /// 補遺番号
  #[serde(rename = "supplement-number")]
  pub supplement_number: Option<NumberOrString>,
  /// バージョン
  pub version: Option<NumberOrString>,
  /// 巻号（ボリューム）
  pub volume: Option<NumberOrString>,

  // Date Variables
  /// アクセス日
  pub accessed: Option<Date>,
  /// 利用可能日
  #[serde(rename = "available-date")]
  pub available_date: Option<Date>,
  /// イベント開催日
  #[serde(rename = "event-date")]
  pub event_date: Option<Date>,
  /// 発行日
  pub issued: Option<Date>,
  /// 原著の発行日
  #[serde(rename = "original-date")]
  pub original_date: Option<Date>,
  /// 提出日
  pub submitted: Option<Date>,

  // Name Variables
  /// 著者リスト
  pub author: Option<Vec<N>>,
  /// 議長
  pub chair: Option<Vec<N>>,
  /// コレクション編集者
  #[serde(rename = "collection-editor")]
  pub collection_editor: Option<Vec<N>>,
  /// 編纂者
  pub compiler: Option<Vec<N>>,
  /// 作曲者
  pub composer: Option<Vec<N>>,
  /// 収録著者（収録誌・書籍の著者）
  #[serde(rename = "container-author")]
  pub container_author: Option<Vec<N>>,
  /// 貢献者
  pub contributor: Option<Vec<N>>,
  /// キュレーター
  pub curator: Option<Vec<N>>,
  /// 監督
  pub director: Option<Vec<N>>,
  /// 編集者
  pub editor: Option<Vec<N>>,
  /// 編集主幹
  #[serde(rename = "editorial-director")]
  pub editorial_director: Option<Vec<N>>,
  /// 編集翻訳者
  #[serde(rename = "editor-translator")]
  pub editor_translator: Option<Vec<N>>,
  /// エグゼクティブプロデューサー
  #[serde(rename = "executive-producer")]
  pub executive_producer: Option<Vec<N>>,
  /// ゲスト
  pub guest: Option<Vec<N>>,
  /// 司会者
  pub host: Option<Vec<N>>,
  /// 挿絵画家
  pub illustrator: Option<Vec<N>>,
  /// インタビュアー
  pub interviewer: Option<Vec<N>>,
  /// ナレーター
  pub narrator: Option<Vec<N>>,
  /// 主催者
  pub organizer: Option<Vec<N>>,
  /// 原著者
  #[serde(rename = "original-author")]
  pub original_author: Option<Vec<N>>,
  /// 演者
  pub performer: Option<Vec<N>>,
  /// プロデューサー
  pub producer: Option<Vec<N>>,
  /// 受取人
  pub recipient: Option<Vec<N>>,
  /// レビュー対象の著者
  #[serde(rename = "reviewed-author")]
  pub reviewed_author: Option<Vec<N>>,
  /// 脚本家
  #[serde(rename = "script-writer")]
  pub script_writer: Option<Vec<N>>,
  /// シリーズ制作者
  #[serde(rename = "series-creator")]
  pub series_creator: Option<Vec<N>>,
  /// 翻訳者
  pub translator: Option<Vec<N>>,
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

/// 数値または文字列の値。
///
/// CSL の Number Variables は整数・小数のいずれの数値も、ページ範囲（例: `"1-10"`）など
/// 数値で表現できない値を保持する文字列も許容する。
/// <https://docs.citationstyles.org/en/stable/specification.html#number-variables>
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum NumberOrString {
  /// 整数値
  Integer(i64),
  /// 小数値
  Float(f64),
  /// 文字列値
  String(String),
}
