//! PDF しおり（アウトライン）エントリの組み立て

use crate::typeset::lowering::HeadingRecord;

/// PDF のしおりに使う見出し。
#[derive(Debug, Clone)]
pub(crate) struct OutlineEntry {
  /// 見出しレベル（ネストの深さに使う）
  pub(crate) level: crate::document::HeadingLevel,
  /// しおりに表示するテキスト（`"{number} {plain title}"`）
  pub(crate) text: String,
}

/// 見出し記録から PDF しおりを文書順に組み立てる。
///
/// 番号があれば表題の前に付ける。
pub(super) fn collect_outline_entries(headings: &[HeadingRecord]) -> Vec<OutlineEntry> {
  return headings
    .iter()
    .map(|info| {
      let text = heading_label(&info.number, &info.title_plain);
      return OutlineEntry {
        level: info.level,
        text,
      };
    })
    .collect();
}

/// 番号とタイトルの空を考慮して表示文字列を組む。
fn heading_label(number: &str, title_plain: &str) -> String {
  if number.is_empty() {
    return title_plain.to_string();
  }
  if title_plain.is_empty() {
    return number.to_string();
  }
  return format!("{number} {title_plain}");
}

#[cfg(test)]
mod tests {
  use super::heading_label;

  #[test]
  fn heading_label_combines_number_and_title() {
    assert_eq!(heading_label("1.2", "Intro"), "1.2 Intro");
    assert_eq!(heading_label("", "Intro"), "Intro");
    assert_eq!(heading_label("1.2", ""), "1.2");
  }
}
