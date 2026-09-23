//! PDF しおり（アウトライン）エントリの組み立て

use crate::{document::HeadingLevel, typeset::lowering::HeadingRecord};

/// PDF のしおりに使う見出し。
#[derive(Debug, Clone)]
pub(crate) struct OutlineEntry {
  /// 見出しレベル（ネストの深さに使う）
  pub(crate) level: HeadingLevel,
  /// しおりに表示するテキスト（[`HeadingRecord::label`]）
  pub(crate) text: String,
}

/// 見出し記録から PDF しおりを文書順に組み立てる。
///
/// 番号があれば表題の前に付ける。
pub(super) fn collect_outline_entries(headings: &[HeadingRecord]) -> Vec<OutlineEntry> {
  return headings
    .iter()
    .map(|info| {
      return OutlineEntry {
        level: info.level,
        text: info.label(),
      };
    })
    .collect();
}
