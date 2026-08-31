//! 脚注のページ単位採番（`crate::style::FootnoteNumbering::PerPage`）の不動点 solver
//!
//! 番号とページ割り当ての循環をこのモジュールに閉じ込める。確定ページ列から次の番号列を得る
//! 算出もこの solver の内部操作なのでここが持つ。

use tracing::debug;

use crate::typeset::{boxes::Page, error::TypesetError, pagination::body::BodyLayout};

/// 脚注のページ単位採番で本文パスを回す上限回数。
///
/// 通常は下見と番号反映の 2 回で収束し、残りはページ境界が動く場合の余裕とする。
const MAX_FOOTNOTE_NUMBERING_PASSES: u32 = 4;

/// 脚注のページ単位採番を不動点まで反復して本文ページを確定する。
///
/// 番号はマーカー幅を通じてページ割り当てを変えうるため、番号を与えて本文を組み直す。
///
/// ページ列から再計算した番号が入力番号と一致すれば、不動点として確定する。
///
/// # Errors
///
/// 本文パスが失敗した場合、または上限回数で収束しない場合にエラーを返す。
pub(super) fn solve_per_page_numbering(
  body_pass: &impl Fn(Option<&[u32]>) -> Result<BodyLayout, TypesetError>,
) -> Result<BodyLayout, TypesetError> {
  // 1 回目は空マップ＝全脚注が通し番号へフォールバックする（＝ページ割り当てを知るための下見）。
  let mut numbers: Vec<u32> = Vec::new();
  let mut pass: u32 = 1;
  loop {
    let layout = body_pass(Some(&numbers))?;
    let next = per_page_footnote_numbers(&layout.pages);
    if next == numbers {
      debug!(pass_count = pass, "脚注のページ単位採番が収束");
      return Ok(layout);
    }
    if pass == MAX_FOOTNOTE_NUMBERING_PASSES {
      return Err(TypesetError::PerPageFootnoteNotConverged {
        passes: MAX_FOOTNOTE_NUMBERING_PASSES,
      });
    }
    numbers = next;
    pass += 1;
  }
}

/// 確定したページ列から、脚注のページ単位表示番号を割り当てる
///
/// # Panics
///
/// 文書中の脚注数、または 1 ページの脚注数が `u32` に収まらない場合にパニックします。
#[must_use]
fn per_page_footnote_numbers(pages: &[Page]) -> Vec<u32> {
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
  use std::cell::RefCell;

  use miette::Diagnostic;

  use super::{BodyLayout, MAX_FOOTNOTE_NUMBERING_PASSES, per_page_footnote_numbers, solve_per_page_numbering};
  use crate::{
    length::Length,
    typeset::boxes::{Page, PlacedFootnote},
  };

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
      content_origin_x: Length::ZERO,
    };
  }

  #[test]
  fn per_page_footnote_passes_stop_at_fixed_point() {
    // Arrange — ページ割り当てが番号に依らず安定している本文パスを模す（実文書の通常ケース）。
    let calls = RefCell::new(0u32);
    let body_pass = |_numbers: Option<&[u32]>| {
      *calls.borrow_mut() += 1;
      return Ok(BodyLayout {
        pages: vec![page_with_footnotes(&[0, 1]), page_with_footnotes(&[2])],
        headings: Vec::new(),
        overflows: Vec::new(),
      });
    };

    // Act
    let layout = solve_per_page_numbering(&body_pass).expect("収束するはず");

    // Assert — 1 回目でページ割り当てを知り、2 回目でページ単位番号を反映して不動点に達する。
    assert_eq!(*calls.borrow(), 2, "実質 2 回で収束するはず");
    assert_eq!(layout.pages.len(), 2);
  }

  #[test]
  fn per_page_footnote_passes_report_diagnostic_when_not_converged() {
    // Arrange — 番号を与えるたびにページ割り当てが変わり続けて収束しない本文パスを模す。
    let calls = RefCell::new(0u32);
    let body_pass = |_numbers: Option<&[u32]>| {
      let call = {
        let mut c = calls.borrow_mut();
        *c += 1;
        *c
      };
      let pages = if call % 2 == 0 {
        vec![page_with_footnotes(&[0]), page_with_footnotes(&[1])]
      } else {
        vec![page_with_footnotes(&[0, 1]), page_with_footnotes(&[])]
      };
      return Ok(BodyLayout {
        pages,
        headings: Vec::new(),
        overflows: Vec::new(),
      });
    };

    // Act — 上限回数で打ち切り、最後の不整合なレイアウトを成功として返さない。
    let error = solve_per_page_numbering(&body_pass).expect_err("収束しない場合は診断を返すはず");

    // Assert
    assert_eq!(*calls.borrow(), MAX_FOOTNOTE_NUMBERING_PASSES, "上限回数で打ち切るはず");
    assert_eq!(
      error.code().expect("診断コードを持つはず").to_string(),
      "typeset::footnote::per_page_not_converged",
      "回避策付きの専用診断になるはず"
    );
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
    let numbers = per_page_footnote_numbers(&[page_with_footnotes(&[])]);

    assert!(numbers.is_empty());
  }
}
