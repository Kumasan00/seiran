//! 脚注のページ単位表示番号割り当て
//!
//! ラベル登録・カウンタ値算出（旧 `CounterRegistry`）は issue #282 で `resolve` クレートへ
//! 移設した。ページ単位表示番号割り当ては、ラベル・カウンタ解決とは無関係の別関心事
//! （`resolve` クレートが持つべき責務ではない）なのでこのファイルに残す。
//!
//! 脚注の出現 index 発番（旧 `CounterRegistry::next_footnote_index` / `footnote_count`
//! フィールド）は、`CounterRegistry` 自体がこのクレートから無くなったことで
//! `impl CounterRegistry` に閉じたメソッドとしては存在できなくなったため、この移設時点で
//! いったん削除した（0 起点で単純増加する出現 index を、呼び出しごとに 1 つ払い出し
//! インクリメントする前の値を返すだけの機能だった）。`typeset::lowering::inline` /
//! `typeset::lowering::table` に呼び出し元が残っているため、Task 7 で `typeset` 側の
//! 置き場所（`CounterRegistry` とは別の小さな struct 等）を再設計して復元する

use crate::layout::Page;

/// 確定したページ列から、脚注のページ単位表示番号を割り当てる（`FootnoteNumbering::PerPage`）
///
/// # Panics
///
/// 文書中の脚注数、または 1 ページの脚注数が `u32` に収まらない場合にパニックします。
#[must_use]
pub fn per_page_footnote_numbers(pages: &[Page]) -> Vec<u32> {
  let mut numbers: Vec<u32> = Vec::new();
  for page in pages {
    // 繰越（前ページからの続き、#227）はこのページで「始まった」脚注ではないので数えない。
    // 数えると (a) 自分自身が本体を置いた前ページの番号を上書きし、(b) このページの本当の
    // 1 個目を 2 番へずらす。
    for (position, footnote) in page.footnotes.iter().filter(|footnote| return !footnote.continued).enumerate() {
      let index = usize::try_from(footnote.index).expect("脚注の出現 index は usize に収まる前提");
      // 目的の index まで通し値で伸ばしてから、配置が分かっている脚注だけを上書きする。
      // 伸ばした途中の穴（未配置の脚注）は通し値のまま残る。
      while numbers.len() <= index {
        let continuous = u32::try_from(numbers.len() + 1).expect("脚注の個数は u32 に収まる前提");
        numbers.push(continuous);
      }
      numbers[index] = u32::try_from(position + 1).expect("1 ページの脚注数は u32 に収まる前提");
    }
  }
  return numbers;
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::layout::PlacedFootnote;

  /// 指定した出現 index の脚注だけを載せたページを作るテストヘルパ
  fn page_with_footnotes(indices: &[u32]) -> Page {
    return page_with_footnote_fragments(&indices.iter().map(|index| return (*index, false)).collect::<Vec<_>>());
  }

  /// 出現 index と「繰越（前ページからの続き）か」の組から 1 ページを作るテストヘルパ
  fn page_with_footnote_fragments(fragments: &[(u32, bool)]) -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: fragments
        .iter()
        .map(|(index, continued)| {
          return PlacedFootnote {
            number: index + 1,
            index: *index,
            continued: *continued,
            blocks: Vec::new(),
          };
        })
        .collect(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
    };
  }

  #[test]
  fn per_page_numbers_restart_from_one_on_each_page() {
    // Arrange
    let pages = vec![
      page_with_footnotes(&[0, 1, 2]),
      page_with_footnotes(&[3, 4]),
    ];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 2, 3, 1, 2]);
  }

  #[test]
  fn per_page_numbers_restart_after_page_without_footnotes() {
    // Arrange
    let pages = vec![
      page_with_footnotes(&[0]),
      page_with_footnotes(&[]),
      page_with_footnotes(&[1]),
    ];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 1]);
  }

  #[test]
  fn per_page_numbers_ignore_carried_over_fragments() {
    // Arrange
    let pages = vec![
      page_with_footnote_fragments(&[(0, false)]),
      page_with_footnote_fragments(&[(0, true), (1, false)]),
    ];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 1]);
  }

  #[test]
  fn per_page_numbers_fill_unplaced_footnotes_with_continuous_value() {
    // Arrange
    let pages = vec![page_with_footnotes(&[0, 2])];

    // Act
    let numbers = per_page_footnote_numbers(&pages);

    // Assert
    assert_eq!(numbers, vec![1, 2, 2]);
  }

  #[test]
  fn per_page_numbers_are_empty_without_footnotes() {
    // Arrange / Act
    let numbers = per_page_footnote_numbers(&[page_with_footnotes(&[])]);

    // Assert
    assert!(numbers.is_empty());
  }
}
