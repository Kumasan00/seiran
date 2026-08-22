//! PDF メタデータを組み立てる。

use chrono::{Datelike, Timelike, Utc};
use krilla::metadata::{DateTime, Metadata};
use seiran_compiler::PublicationMetadata;

/// [`PublicationMetadata`] から Krilla のメタデータを構築する。
pub(crate) fn build_metadata(metadata: &PublicationMetadata) -> Metadata {
  let now = Utc::now();
  #[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "chrono が各暦要素を暦上の範囲内で返すことを保証する"
  )]
  let time = DateTime::new(now.year() as u16)
    .month(now.month() as u8)
    .day(now.day() as u8)
    .hour(now.hour() as u8)
    .minute(now.minute() as u8);
  let mut out = Metadata::new()
    .title(metadata.title.clone())
    .creation_date(time)
    .creator("seiran".to_string())
    .producer("seiran".to_string());
  if let Some(author) = &metadata.author {
    out = out.authors(vec![author.clone()]);
  }
  if let Some(subject) = &metadata.subject {
    out = out.description(subject.clone());
  }
  if let Some(language) = &metadata.language {
    out = out.language(language.clone());
  }
  if let Some(keywords) = &metadata.keywords {
    out = out.keywords(keywords.clone());
  }
  return out;
}
