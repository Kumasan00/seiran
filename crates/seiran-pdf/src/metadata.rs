//! PDF メタデータを組み立てる。

use krilla::metadata::{DateTime, Metadata};
use seiran_compiler::PublicationMetadata;
use time::OffsetDateTime;

/// [`PublicationMetadata`] から Krilla のメタデータを構築する。
pub(crate) fn build_metadata(metadata: &PublicationMetadata) -> Metadata {
  let now = OffsetDateTime::now_utc();
  #[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "現在時刻の年は 1970 以降で、time は large-dates なしでは 9999 年までしか表さないので u16 に収まる"
  )]
  let creation_date = DateTime::new(now.year() as u16)
    .month(u8::from(now.month()))
    .day(now.day())
    .hour(now.hour())
    .minute(now.minute());
  let mut out = Metadata::new()
    .title(metadata.title.clone())
    .creation_date(creation_date)
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
