//! PDF しおり（アウトライン）エントリの組み立て

use document::{DocNode, collect_headings, inline_nodes_to_plain_text};
use pdf_gen::OutlineEntry;

/// Document IR の見出しから PDF しおり用の [`pdf_gen::OutlineEntry`] を文書順に組み立てる。
///
/// テキストは `"{number} {plain title}"`（番号が空なら表題のみ）。見出しの収集は意味側の
/// 単一ソース [`document::collect_headings`] に委譲する（目次生成と同じソースを使う）。
pub(super) fn collect_outline_entries(doc_nodes: &[DocNode]) -> Vec<OutlineEntry> {
  return collect_headings(doc_nodes)
    .into_iter()
    .map(|info| {
      let plain = inline_nodes_to_plain_text(info.title);
      let text = heading_label(info.number, &plain);
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
