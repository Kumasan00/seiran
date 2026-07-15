//! PDF メタデータの組み立て。
//!
//! `config.document` を Krilla の [`Metadata`] に詰め替える純粋なデータ変換層で、
//! フォント・レイアウト・PDF 描画には依存しない。

use chrono::{Datelike, Timelike, Utc};
use config::Config;
use krilla::metadata::{DateTime, Metadata};

/// `config.document` から PDF メタデータを構築します。
///
/// `/Title` は `document.title` を優先し、未設定なら `output.name` にフォールバックします。
pub(crate) fn build_metadata(config: &Config) -> Metadata {
  let now = Utc::now();
  #[allow(clippy::cast_sign_loss)]
  let time = DateTime::new(now.year() as u16)
    .month(now.month() as u8)
    .day(now.day() as u8)
    .hour(now.hour() as u8)
    .minute(now.minute() as u8);
  let title = config.document.title.clone().unwrap_or_else(|| config.output.name.clone());
  let mut metadata = Metadata::new()
    .title(title)
    .creation_date(time)
    .creator("seiran".to_string())
    .producer("seiran".to_string());
  if let Some(author) = &config.document.author {
    metadata = metadata.authors(vec![author.clone()]);
  }
  if let Some(subject) = &config.document.subject {
    metadata = metadata.description(subject.clone());
  }
  if let Some(language) = &config.document.language {
    metadata = metadata.language(language.clone());
  }
  if let Some(keywords) = &config.document.keywords {
    metadata = metadata.keywords(keywords.clone());
  }
  return metadata;
}
