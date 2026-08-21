//! 脚注のページ単位採番（`crate::style::FootnoteNumbering::PerPage`）の不動点 solver
//!
//! 番号とページ割り当ての循環をこのモジュールに閉じ込める。

use tracing::debug;

use super::body::BodyLayout;
use crate::typeset::{error::TypesetError, lowering::per_page_footnote_numbers};

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
      debug!(pass, "脚注のページ単位採番が収束しました");
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

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use miette::Diagnostic;

  use super::{BodyLayout, MAX_FOOTNOTE_NUMBERING_PASSES, solve_per_page_numbering};
  use crate::typeset::boxes::{Page, PlacedFootnote};

  /// 指定した出現 index の脚注だけを持つ 1 ページを作るテストヘルパ
  fn page_with_footnotes(indices: &[u32]) -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: indices
        .iter()
        .map(|index| {
          return PlacedFootnote {
            number: index + 1,
            index: *index,
            continued: false,
            blocks: Vec::new(),
          };
        })
        .collect(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
      content_origin_x: crate::length::Length::ZERO,
    };
  }

  #[test]
  fn per_page_footnote_passes_stop_at_fixed_point() {
    // Arrange — ページ割り当てが番号に依らず安定している本文パスを模す（実文書の通常ケース）。
    let calls = RefCell::new(0_u32);
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
    let calls = RefCell::new(0_u32);
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
}
