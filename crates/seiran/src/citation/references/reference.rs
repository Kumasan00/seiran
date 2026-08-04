//! CSL (Citation Style Language) 文献情報のデータモデル。

use std::{collections::HashMap, fmt};

use serde::{
  Deserialize, Deserializer, Serialize,
  de::{Error, MapAccess, Visitor},
};

use crate::citation::references::{date::Date, name::Name};

/// 参照定義ファイル全体を表す構造体
///
/// トップレベルのテーブルキーを参照 ID として保持し、空・空白のみ・重複する ID を拒否する。
#[derive(Debug)]
pub struct References(pub HashMap<String, Reference>);

impl std::ops::Deref for References {
  type Target = HashMap<String, Reference>;

  /// 内包する参照定義マップへの参照を返す（`HashMap` の API を透過的に利用するため）。
  fn deref(&self) -> &Self::Target { return &self.0; }
}

impl<'de> Deserialize<'de> for References {
  /// 参照 ID をキーとするトップレベルテーブルをデシリアライズする。
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let references = deserialize_unique_references(deserializer)?;
    return Ok(References(references));
  }
}

/// 参照定義マップを参照 ID の検証付きでデシリアライズする。
///
/// 空・空白のみの参照 ID と重複キーを拒否する。
fn deserialize_unique_references<'de, D>(deserializer: D) -> Result<HashMap<String, Reference>, D::Error>
where
  D: Deserializer<'de>,
{
  /// 参照 ID を検証する `HashMap` 用の `Visitor`。
  struct UniqueReferencesVisitor;

  impl<'de> Visitor<'de> for UniqueReferencesVisitor {
    type Value = HashMap<String, Reference>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
      return formatter.write_str("参照 ID をキーとするテーブル");
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
      A: MapAccess<'de>,
    {
      let mut map: HashMap<String, Reference> = HashMap::with_capacity(access.size_hint().unwrap_or(0));
      while let Some((id, reference)) = access.next_entry::<String, Reference>()? {
        if id.is_empty() {
          return Err(A::Error::custom("参照 ID（テーブルキー）に空文字列は使用できません"));
        }
        if id.trim().is_empty() {
          return Err(A::Error::custom(format!("参照 ID（テーブルキー）に空白のみの文字列は使用できません: {id:?}")));
        }
        if map.contains_key(&id) {
          return Err(A::Error::custom(format!("参照 ID が重複しています: {id}")));
        }
        map.insert(id, reference);
      }
      return Ok(map);
    }
  }

  return deserializer.deserialize_map(UniqueReferencesVisitor);
}

/// 個々の参照定義を表す構造体
///
/// CSL (Citation Style Language) に基づく文献情報を保持する。
/// 参照 ID は keyed-table 形式のテーブルキーとして保持されるため、本構造体には持たない。
/// <https://docs.citationstyles.org/en/stable/specification.html#appendix-iv-variables>
// `reference_type` は `#[serde(rename = "type")]` により CSL の `type` キーへ写像するため、
// struct 名との重複は意図的。旧 citation crate では公開 API のため対象外だったが、seiran へ
// 吸収され非公開 module 化されたことで `clippy::struct_field_names`（pedantic、実効可視性
// ベース）が新たに発火する。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_field_names)]
pub struct Reference {
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
  pub author: Option<Vec<Name>>,
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
  #[serde(rename = "editor-translator")]
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
#[derive(Debug, Serialize, Deserialize)]
pub enum ReferenceType {
  /// 記事（学術誌以外の一般的な記事）
  #[serde(rename = "article")]
  Article,
  /// 学術誌の記事
  #[serde(rename = "article-journal")]
  ArticleJournal,
  /// 雑誌記事
  #[serde(rename = "article-magazine")]
  ArticleMagazine,
  /// 新聞記事
  #[serde(rename = "article-newspaper")]
  ArticleNewspaper,
  /// 法案
  #[serde(rename = "bill")]
  Bill,
  /// 書籍
  #[serde(rename = "book")]
  Book,
  /// 放送番組
  #[serde(rename = "broadcast")]
  Broadcast,
  /// 書籍の章
  #[serde(rename = "chapter")]
  Chapter,
  /// 古典（著者・出版年が定まらない古典作品）
  #[serde(rename = "classic")]
  Classic,
  /// 論文集・作品集
  #[serde(rename = "collection")]
  Collection,
  /// データセット
  #[serde(rename = "dataset")]
  Dataset,
  /// 文書（他の種別に分類できない一般文書）
  #[serde(rename = "document")]
  Document,
  /// 事典・辞書等の項目（種別不特定）
  #[serde(rename = "entry")]
  Entry,
  /// 辞書項目
  #[serde(rename = "entry-dictionary")]
  EntryDictionary,
  /// 百科事典項目
  #[serde(rename = "entry-encyclopedia")]
  EntryEncyclopedia,
  /// イベント（展示会・式典等）
  #[serde(rename = "event")]
  Event,
  /// 図表
  #[serde(rename = "figure")]
  Figure,
  /// 図版・グラフィック作品
  #[serde(rename = "graphic")]
  Graphic,
  /// 公聴会記録
  #[serde(rename = "hearing")]
  Hearing,
  /// インタビュー
  #[serde(rename = "interview")]
  Interview,
  /// 判例
  #[serde(rename = "legal_case")]
  LegalCase,
  /// 法令
  #[serde(rename = "legislation")]
  Legislation,
  /// 未刊行原稿
  #[serde(rename = "manuscript")]
  Manuscript,
  /// 地図
  #[serde(rename = "map")]
  Map,
  /// 映画
  #[serde(rename = "motion_picture")]
  MotionPicture,
  /// 楽譜
  #[serde(rename = "musical_score")]
  MusicalScore,
  /// パンフレット
  #[serde(rename = "pamphlet")]
  Pamphlet,
  /// 学会発表論文
  #[serde(rename = "paper-conference")]
  PaperConference,
  /// 特許
  #[serde(rename = "patent")]
  Patent,
  /// 上演・公演
  #[serde(rename = "performance")]
  Performance,
  /// 定期刊行物
  #[serde(rename = "periodical")]
  Periodical,
  /// 私信
  #[serde(rename = "personal_communication")]
  PersonalCommunication,
  /// （ウェブ上の）投稿
  #[serde(rename = "post")]
  Post,
  /// ブログ記事
  #[serde(rename = "post-weblog")]
  PostWeblog,
  /// 規則
  #[serde(rename = "regulation")]
  Regulation,
  /// 報告書
  #[serde(rename = "report")]
  Report,
  /// レビュー
  #[serde(rename = "review")]
  Review,
  /// 書評
  #[serde(rename = "review-book")]
  ReviewBook,
  /// ソフトウェア
  #[serde(rename = "software")]
  Software,
  /// 楽曲
  #[serde(rename = "song")]
  Song,
  /// 講演
  #[serde(rename = "speech")]
  Speech,
  /// 規格
  #[serde(rename = "standard")]
  Standard,
  /// 学位論文
  #[serde(rename = "thesis")]
  Thesis,
  /// 条約
  #[serde(rename = "treaty")]
  Treaty,
  /// ウェブページ
  #[serde(rename = "webpage")]
  Webpage,
}

/// 数値または文字列の値。
///
/// CSL の Number Variables は整数・小数のいずれの数値も、ページ範囲（例: `"1-10"`）など
/// 数値で表現できない値を保持する文字列も許容する。
/// <https://docs.citationstyles.org/en/stable/specification.html#number-variables>
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NumberOrString {
  /// 整数値
  Integer(i64),
  /// 小数値
  Float(f64),
  /// 文字列値
  String(String),
}
