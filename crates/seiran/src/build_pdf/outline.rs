//! PDF しおり（アウトライン）エントリの組み立て

use lowering::HeadingRecord;
use pdf_gen::OutlineEntry;

/// lowering が収集した見出し記録から PDF しおり用の [`pdf_gen::OutlineEntry`] を文書順に組み立てる。
///
/// テキストは `"{number} {plain title}"`（番号が空なら表題のみ）。見出しの収集は
/// `lowering::lower_nodes_with_headings`（目次生成と同じソース）に委譲する。
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

/// 「番号 タイトル」の表示文字列を組む（番号・タイトルの空を考慮）。しおり・目次で共用する。
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
