//! 脚注のページ単位採番（`config::FootnoteNumbering::PerPage`）の不動点 solver
//!
//! ページ依存処理のうち循環が残るのはページ単位の脚注採番だけで、この module がその状態
//! （番号 → マーカー寸法 → 行分割 → ページ分割 → ページごとの番号）を所有する
//! （`docs/redesign-from-scratch.md`「ページ単位の脚注採番」）。他の phase は
//! `compile_project` の phase graph が順序どおり 1 回ずつ呼ぶ。

use tracing::debug;

use super::{body::BodyLayout, error::BuildPdfError};

/// 脚注のページ単位採番で本文パスを回す上限回数。
///
/// 1 回目は通し番号で組んで脚注のページ割り当てを知り、2 回目でページ単位番号を反映する。
/// 実質はここで収束するので、残りは番号の変化がページ割り当てを揺らすケース用の余裕。
const MAX_FOOTNOTE_NUMBERING_PASSES: u32 = 4;

/// 脚注のページ単位採番を不動点まで反復して本文ページを確定する。
///
/// ページ単位採番は「番号 → マーカーの桁数 → マーカー幅 → 行分割 → ページ分割 → 脚注のページ
/// 割り当て → 番号」と循環している。`break_pages` はフォント非依存の純粋パスで、ページ確定後に
/// マーカーのグリフを作り直すことはできない（アーキテクチャ上の不変条件）ため、番号を与えて
/// 組み直す反復で解く。
///
/// 各パスは「そのパスで表示した番号」で組まれたページ列を返す。そのページ列から番号を割り当て
/// 直しても同じ番号になれば、表示とページ割り当てが一致した＝不動点なのでそこで止める。
/// 脚注のない文書は 1 回目で（マップが空のまま）収束する。
///
/// 反復が成り立つのは、番号が**表示値しか変えない**から。どの脚注が存在するか・文書順は番号に
/// 依存しないので、出現 index は全パスで同じ脚注を指し続け、マップがパス間で整合する。
///
/// # Errors
///
/// - `body_pass` が失敗した場合（lowering・画像サイズ確定のエラー）はそのまま伝播する
/// - [`MAX_FOOTNOTE_NUMBERING_PASSES`] 回で収束しなかった場合は
///   [`BuildPdfError::PerPageFootnoteNotConverged`] を返す（不整合な結果を成功扱いしない）
pub(super) fn solve_per_page_numbering(
  body_pass: &impl Fn(Option<&[u32]>) -> miette::Result<BodyLayout>,
) -> miette::Result<BodyLayout> {
  // 1 回目は空マップ＝全脚注が通し番号へフォールバックする（＝ページ割り当てを知るための下見）。
  let mut numbers: Vec<u32> = Vec::new();
  let mut pass: u32 = 1;
  loop {
    let layout = body_pass(Some(&numbers))?;
    let next = typeset::per_page_footnote_numbers(&layout.pages);
    if next == numbers {
      debug!(pass, "脚注のページ単位採番が収束しました");
      return Ok(layout);
    }
    if pass == MAX_FOOTNOTE_NUMBERING_PASSES {
      return Err(
        BuildPdfError::PerPageFootnoteNotConverged {
          passes: MAX_FOOTNOTE_NUMBERING_PASSES,
        }
        .into(),
      );
    }
    numbers = next;
    pass += 1;
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::cell::RefCell;

  use super::{BodyLayout, MAX_FOOTNOTE_NUMBERING_PASSES, solve_per_page_numbering};

  /// 指定した出現 index の脚注だけを持つ 1 ページを作るテストヘルパ
  fn page_with_footnotes(indices: &[u32]) -> model::Page {
    return model::Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: indices
        .iter()
        .map(|index| {
          return model::PlacedFootnote {
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
      });
    };

    // Act — 上限回数で打ち切り、最後の不整合なレイアウトを成功として返さない。
    let report = solve_per_page_numbering(&body_pass).expect_err("収束しない場合は診断を返すはず");

    // Assert
    assert_eq!(*calls.borrow(), MAX_FOOTNOTE_NUMBERING_PASSES, "上限回数で打ち切るはず");
    assert_eq!(
      report.code().expect("診断コードを持つはず").to_string(),
      "build::footnote::per_page_not_converged",
      "回避策付きの専用診断になるはず"
    );
  }
}
