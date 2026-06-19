//! `read_references::Reference`（CSL-JSON 型付きモデル）から `hayagriva::Entry` への変換ブリッジ。
//!
//! Seiran は CSL-JSON 由来の `read_references` モデルを正とし、CSL 整形エンジン hayagriva は
//! 独自の `Entry` モデルを持つ。両モデルの差はこのファイル 1 箇所に隔離する（モデルは二重になるが、
//! 代わりに同梱 `ieee.csl` の実効化・正確な採番/整列/曖昧性回避を hayagriva に委譲できる）。
//!
//! マップするのは常用サブセット（type / title / author / editor / issued / container-title /
//! publisher / volume / issue / page / doi / url / isbn / edition）に限り、未対応フィールドは
//! 当面スキップする（段階拡張）。

use hayagriva::{
  Entry,
  types::{
    Date as HayaDate, EntryType, FormatString, MaybeTyped, Numeric, PageRanges, Person, Publisher, QualifiedUrl,
  },
};
use read_references::{Date, Name, NumberOrString, Reference, ReferenceType};

/// `Reference` を hayagriva の `Entry` に変換する。
///
/// `id` は参照定義のキー（`references` マップのキー）で、hayagriva の cite key となる。
pub(crate) fn to_entry(id: &str, reference: &Reference) -> Entry {
  let entry_type = to_entry_type(&reference.reference_type);
  let mut entry = Entry::new(id, entry_type);

  if let Some(title) = &reference.title {
    entry.set_title(FormatString::from(title.clone()));
  }
  if let Some(authors) = &reference.author {
    entry.set_authors(authors.iter().map(to_person).collect());
  }
  if let Some(editors) = &reference.editor {
    entry.set_editors(editors.iter().map(to_person).collect());
  }
  if let Some(date) = reference.issued.as_ref().and_then(to_date) {
    entry.set_date(date);
  }
  if let Some(volume) = &reference.volume {
    entry.set_volume(to_numeric(volume));
  }
  if let Some(issue) = &reference.issue {
    entry.set_issue(to_numeric(issue));
  }
  if let Some(edition) = &reference.edition {
    entry.set_edition(to_numeric(edition));
  }
  if let Some(page) = &reference.page {
    entry.set_page_range(to_page_ranges(page));
  }
  if reference.publisher.is_some() || reference.publisher_place.is_some() {
    entry.set_publisher(to_publisher(reference.publisher.as_deref(), reference.publisher_place.as_deref()));
  }
  if let Some(doi) = &reference.doi {
    entry.set_doi(doi.clone());
  }
  if let Some(isbn) = &reference.isbn {
    entry.set_isbn(isbn.clone());
  }
  if let Some(url) = reference.url.as_ref().and_then(|u| u.parse::<QualifiedUrl>().ok()) {
    entry.set_url(url);
  }
  // container-title（収録誌・収録書名）は hayagriva では親 Entry として表現する。
  if let Some(container) = &reference.container_title {
    let mut parent = Entry::new(&format!("{id}-container"), parent_entry_type(entry_type));
    parent.set_title(FormatString::from(container.clone()));
    entry.set_parents(vec![parent]);
  }

  return entry;
}

/// `ReferenceType`（CSL の type）を hayagriva の `EntryType` に対応づける。
///
/// 明確に対応する常用型のみを個別にマップし、それ以外は `Misc` にフォールバックする。
fn to_entry_type(reference_type: &ReferenceType) -> EntryType {
  return match reference_type {
    ReferenceType::Article
    | ReferenceType::ArticleJournal
    | ReferenceType::ArticleMagazine
    | ReferenceType::PaperConference
    | ReferenceType::Review
    | ReferenceType::ReviewBook
    | ReferenceType::Interview => EntryType::Article,
    ReferenceType::ArticleNewspaper => EntryType::Newspaper,
    ReferenceType::Book | ReferenceType::Classic | ReferenceType::Pamphlet => EntryType::Book,
    ReferenceType::Chapter => EntryType::Chapter,
    ReferenceType::Collection => EntryType::Anthology,
    ReferenceType::Entry => EntryType::Entry,
    ReferenceType::EntryDictionary | ReferenceType::EntryEncyclopedia => EntryType::Reference,
    ReferenceType::Figure | ReferenceType::Graphic => EntryType::Artwork,
    ReferenceType::Manuscript => EntryType::Manuscript,
    ReferenceType::Patent => EntryType::Patent,
    ReferenceType::Performance | ReferenceType::Speech | ReferenceType::Event => EntryType::Performance,
    ReferenceType::Periodical => EntryType::Periodical,
    ReferenceType::Post | ReferenceType::PostWeblog => EntryType::Post,
    ReferenceType::Report => EntryType::Report,
    ReferenceType::Song => EntryType::Audio,
    ReferenceType::Broadcast | ReferenceType::MotionPicture => EntryType::Video,
    ReferenceType::Thesis => EntryType::Thesis,
    ReferenceType::Webpage => EntryType::Web,
    ReferenceType::Bill
    | ReferenceType::Hearing
    | ReferenceType::Legislation
    | ReferenceType::Regulation
    | ReferenceType::Treaty => EntryType::Legislation,
    ReferenceType::LegalCase => EntryType::Case,
    ReferenceType::Dataset
    | ReferenceType::Document
    | ReferenceType::Map
    | ReferenceType::MusicalScore
    | ReferenceType::PersonalCommunication
    | ReferenceType::Software
    | ReferenceType::Standard => EntryType::Misc,
  };
}

/// 親 Entry（container-title）の型を子 Entry の型から決める。
fn parent_entry_type(child: EntryType) -> EntryType {
  return match child {
    EntryType::Chapter | EntryType::Anthos => EntryType::Book,
    _ => EntryType::Periodical,
  };
}

/// `read_references::Name` を hayagriva の `Person` に変換する。
///
/// 個人著者は `family`/`given`/`suffix` を、組織著者は `literal` を名前に充てる。hayagriva の
/// `prefix` は非ドロップ粒子（'van' 等）に対応するため、`non_dropping_particle` を優先して使う。
fn to_person(name: &Name) -> Person {
  return match name {
    Name::Personal {
      family,
      given,
      dropping_particle,
      non_dropping_particle,
      suffix,
    } => Person {
      name: family.clone(),
      given_name: given.clone(),
      prefix: non_dropping_particle.clone().or_else(|| dropping_particle.clone()),
      suffix: suffix.clone(),
      comma_suffix: false,
      alias: None,
    },
    Name::Organization { literal } => Person {
      name: literal.clone(),
      given_name: None,
      prefix: None,
      suffix: None,
      comma_suffix: false,
      alias: None,
    },
  };
}

/// `read_references::Date`（CSL date-parts）を hayagriva の `Date` に変換する。
///
/// CSL の月/日は 1 始まり（month 1-12 / day 1-31）だが、hayagriva は 0 始まり（month 0-11 /
/// day 0-30）なので 1 を引いて補正する。date-parts や年が欠けている場合は `None` を返す。
fn to_date(date: &Date) -> Option<HayaDate> {
  let parts = date.date_parts.as_ref()?.first()?;
  let year = match parts.first()? {
    read_references::DatePart::Number(n) => i32::try_from(*n).ok()?,
    read_references::DatePart::String(s) => s.parse::<i32>().ok()?,
  };
  let mut result = HayaDate::from_year(year);
  if let Some(read_references::DatePart::Number(month)) = parts.get(1) {
    result.month = u8::try_from(month.saturating_sub(1)).ok();
  }
  if let Some(read_references::DatePart::Number(day)) = parts.get(2) {
    result.day = u8::try_from(day.saturating_sub(1)).ok();
  }
  return Some(result);
}

/// `NumberOrString` を hayagriva の数値型 `MaybeTyped<Numeric>` に変換する。
///
/// 整数は型付き `Numeric`、浮動小数・文字列は整形済み文字列としてそのまま保持する。
fn to_numeric(value: &NumberOrString) -> MaybeTyped<Numeric> {
  return match value {
    NumberOrString::Integer(n) => MaybeTyped::Typed(Numeric::new(i32::try_from(*n).unwrap_or_default())),
    NumberOrString::Float(f) => MaybeTyped::String(f.to_string()),
    NumberOrString::String(s) => MaybeTyped::String(s.clone()),
  };
}

/// `NumberOrString` をページ範囲 `MaybeTyped<PageRanges>` に変換する。
///
/// 文字列（"1-10" 等）はページ範囲としてのパースを試み、失敗時はそのまま文字列として保持する。
fn to_page_ranges(value: &NumberOrString) -> MaybeTyped<PageRanges> {
  return match value {
    NumberOrString::Integer(n) => MaybeTyped::Typed(PageRanges::from(i32::try_from(*n).unwrap_or_default())),
    NumberOrString::Float(f) => MaybeTyped::String(f.to_string()),
    NumberOrString::String(s) => {
      s.parse::<PageRanges>().map_or_else(|_| MaybeTyped::String(s.clone()), MaybeTyped::Typed)
    },
  };
}

/// 出版社名・出版地から hayagriva の `Publisher` を作る。
fn to_publisher(name: Option<&str>, location: Option<&str>) -> Publisher {
  return Publisher::new(
    name.map(|n| FormatString::from(n.to_string())),
    location.map(|l| FormatString::from(l.to_string())),
  );
}

#[cfg(test)]
mod tests {
  use hayagriva::types::EntryType;

  use super::to_entry;
  use crate::test_fixtures::sample_references;

  #[test]
  fn to_entry_maps_book_type_title_author_date() {
    // Arrange
    let references = sample_references();
    let reference = references.get("kwan2014").expect("book エントリがあるはず");

    // Act
    let entry = to_entry("kwan2014", reference);

    // Assert
    assert_eq!(entry.entry_type(), &EntryType::Book);
    assert_eq!(entry.title().map(ToString::to_string).as_deref(), Some("Crazy Rich Asians"));
    assert_eq!(entry.authors().map(<[_]>::len), Some(1), "著者 1 名が変換されるはず");
    assert!(entry.date().is_some(), "issued から date が設定されるはず");
  }

  #[test]
  fn to_entry_maps_article_container_as_parent() {
    // Arrange
    let references = sample_references();
    let reference = references.get("doe2020").expect("article エントリがあるはず");

    // Act
    let entry = to_entry("doe2020", reference);

    // Assert
    assert_eq!(entry.entry_type(), &EntryType::Article);
    assert_eq!(entry.parents().len(), 1, "container-title は親 Entry に変換されるはず");
    assert_eq!(
      entry.parents()[0].title().map(ToString::to_string).as_deref(),
      Some("Journal of Things"),
      "親 Entry のタイトルは収録誌名のはず"
    );
  }
}
