//! 段落の行列に対する配置計画（#395）— 純粋関数・データのみ。`PageComposer` には依存しない。
//!
//! ベースライン送り・脚注予約・widow / orphan 補正までをここで決め、結果は [`LinePlacement`] の列として
//! 返すだけにする。計画を実際のページへ確定させる（リージョンを進める・脚注を積む・はみ出しを記録する）のは
//! 親 module の [`super::place_paragraph`] の責務。計画は widow / orphan 補正で何度も立て直されるので、
//! 一度きりであるべき記録をここで作ると重複する（#382）。

use super::{
  MIN_LINES_AT_BREAK,
  footnote_packing::{FootnoteCharges, FootnoteDemand, LineFootnoteFit, fit_line_footnotes, footnote_area_full},
};
use crate::{length::Length, typeset::boxes::Line};

/// 段落 1 行の配置計画（純粋な幾何判定の結果）
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LinePlacement {
  /// 行のベースライン（ページ上端からの距離、pt）
  pub(super) baseline: Length,
  /// この行から新しいリージョン（次段 / 次ページ）が始まるか
  pub(super) starts_region: bool,
  /// この行を確定した時点でのリージョンの脚注予約高さ（pt）。新リージョンが始まった行は
  /// そのリージョンで最初の予約（＝この行自身の脚注ぶんのみ）になる。
  /// [`super::place_paragraph`] が確定ループで `composer.region_footnote_height` へそのまま反映する。
  pub(super) reserved_after: Length,
  /// この行の脚注ごとに、この行が乗るリージョンへ置く行数（行の脚注と同順・同長。脚注が無ければ空）
  pub(super) own_splits: Vec<usize>,
  /// この行の脚注群が空のリージョンにも収まらず、はみ出したまま置かれるか（#382）。
  /// 計画は widow / orphan 補正で何度も立て直されるので、ここでは事実を載せるだけにして、
  /// 警告は確定した計画を配置する [`super::place_paragraph`] だけが組み立てる
  pub(super) overflowed: bool,
}

/// 強制改リージョン点（`forced`）を尊重しつつ、貪欲にベースラインを送って各行を配置する（純粋関数）
#[allow(clippy::too_many_arguments)]
fn place_lines(
  lines: &[Line],
  y0: Length,
  cursor_at_edge: bool,
  leading: Length,
  margin_top: Length,
  page_limit: Length,
  forced: &[bool],
  demands: &[Vec<FootnoteDemand>],
  initial_reserved: Length,
  charges: FootnoteCharges,
  carry_pending: bool,
) -> (Vec<LinePlacement>, bool) {
  let mut plan = Vec::with_capacity(lines.len());
  let mut baseline = y0;
  let mut prev_depth: Option<Length> = None;
  let mut reserved = initial_reserved;
  for (i, line) in lines.iter().enumerate() {
    match prev_depth {
      // 段落先頭行: 直前が底辺基準ブロックならアセント分下げる
      None => {
        if cursor_at_edge {
          baseline += line.height;
        }
      },
      // 2 行目以降: leading か「前の行の深さ + この行の高さ」の大きい方だけ送る
      Some(depth) => {
        baseline += leading.max(depth + line.height);
      },
    }
    let mut fit = if forced[i] {
      LineFootnoteFit::Rejected
    } else {
      fit_line_footnotes(&demands[i], reserved, baseline + line.depth, page_limit, charges)
    };
    let starts_region = matches!(fit, LineFootnoteFit::Rejected);
    if starts_region {
      if carry_pending {
        // 次リージョンの脚注エリアは繰越で埋まる。どれだけ埋まるかを詰めるまでこの行の
        // ベースラインは決められないので、ここで計画を打ち切る（呼び出し元が seed して計画し直す）
        return (plan, true);
      }
      baseline = margin_top;
      fit = fit_line_footnotes(&demands[i], Length::ZERO, baseline + line.depth, page_limit, charges);
    }
    let split_here = matches!(fit, LineFootnoteFit::Split(..));
    let mut overflowed = false;
    let (reserved_after, own_splits) = match fit {
      LineFootnoteFit::Full(area) => (area, demands[i].iter().map(FootnoteDemand::line_count).collect()),
      LineFootnoteFit::Split(area, splits) => (area, splits),
      // 空のリージョンでも収まらない病的ケース（脚注の先頭 1 行がページ全高を超える等）。
      // 次リージョンへ送っても改善しないので、オーバーフローを許容してそのまま置く
      LineFootnoteFit::Rejected => {
        overflowed = !demands[i].is_empty();
        (
          footnote_area_full(&demands[i], Length::ZERO, charges),
          demands[i].iter().map(FootnoteDemand::line_count).collect(),
        )
      },
    };
    reserved = reserved_after;
    plan.push(LinePlacement {
      baseline,
      starts_region,
      reserved_after,
      own_splits,
      overflowed,
    });
    prev_depth = Some(line.depth);
    if split_here {
      return (plan, true);
    }
  }
  return (plan, false);
}

/// 配置計画から widow/orphan 違反を 1 つ検出し、追加すべき強制改リージョン点を返す（純粋関数）
fn pick_correction(
  plan: &[LinePlacement],
  min_lines: usize,
  is_paragraph_start: bool,
  is_paragraph_end: bool,
) -> Option<usize> {
  let n = plan.len();
  if n < 2 {
    return None;
  }
  // orphan: 先頭リージョン（index 0 から最初の改リージョンまで）の行数が最小行数未満
  // （先頭行が既にリージョン先頭なら回避不能なので補正しない）
  if is_paragraph_start
    && let Some(first_break) = (1..n).find(|&i| return plan[i].starts_region)
    && first_break < min_lines
    && !plan[0].starts_region
  {
    return Some(0);
  }
  if !is_paragraph_end {
    return None;
  }
  // widow: 末尾リージョン（最後の改リージョンから末尾まで）の行数が最小行数未満
  if let Some(last_break) = (1..n).rev().find(|&i| return plan[i].starts_region)
    && n - last_break < min_lines
  {
    // 前側に最小行数を残せるなら末尾 min_lines 行だけを送る。残せない短い段落は全体を送る
    let target = if n >= 2 * min_lines { n - min_lines } else { 0 };
    // 全体を送っても先頭が既にリージョン先頭なら回避不能
    if target == 0 && plan[0].starts_region {
      return None;
    }
    return Some(target);
  }
  return None;
}

/// 段落の行列を現在のカーソルから前から順に配置する計画を立てる（純粋関数・widow/orphan 制御込み）
#[allow(clippy::too_many_arguments)]
pub(super) fn plan_paragraph_lines(
  lines: &[Line],
  y0: Length,
  cursor_at_edge: bool,
  leading: Length,
  margin_top: Length,
  page_limit: Length,
  demands: &[Vec<FootnoteDemand>],
  initial_reserved: Length,
  charges: FootnoteCharges,
  is_paragraph_start: bool,
  carry_pending: bool,
) -> (Vec<LinePlacement>, bool) {
  let mut forced = vec![false; lines.len()];
  loop {
    let (plan, truncated) = place_lines(
      lines,
      y0,
      cursor_at_edge,
      leading,
      margin_top,
      page_limit,
      &forced,
      demands,
      initial_reserved,
      charges,
      carry_pending,
    );
    // 打ち切られた計画の末尾は段落の末尾ではない（続きは繰越を詰めてから計画し直す）
    let is_paragraph_end = !truncated;
    match pick_correction(&plan, MIN_LINES_AT_BREAK, is_paragraph_start, is_paragraph_end) {
      // 新しい補正点なら強制して再フロー
      Some(idx) if !forced[idx] => forced[idx] = true,
      // 補正不要、または前進しない（回避不能）なら確定
      _ => return (plan, truncated),
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{FootnoteCharges, FootnoteDemand, LinePlacement, plan_paragraph_lines};
  use crate::{length::Length, typeset::boxes::Line};

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// 高さ 8・深さ 2 の単純な行（純粋関数テスト用）
  fn test_line() -> Line {
    return Line {
      boxes: Vec::new(),
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
      is_last: false,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }

  /// 脚注を持たない `count` 行ぶんの需要（[`super::place_lines`] / [`plan_paragraph_lines`] のテスト用）
  fn no_footnotes(count: usize) -> Vec<Vec<FootnoteDemand>> { return (0..count).map(|_| return Vec::new()).collect(); }

  /// 課金ゼロの脚注パラメータ（脚注を使わない計画テスト用）
  fn no_charges() -> FootnoteCharges {
    return FootnoteCharges {
      top_margin: Length::ZERO,
      rule_thickness: Length::ZERO,
      rule_gap: Length::ZERO,
    };
  }

  #[test]
  fn plan_leaves_fitting_paragraph_untouched() {
    // Arrange
    let lines = vec![test_line(), test_line(), test_line()];

    // Act
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      pt(10.0),
      false,
      pt(12.0),
      pt(10.0),
      pt(50.0),
      &no_footnotes(3),
      Length::ZERO,
      no_charges(),
      true,
      false,
    );

    // Assert
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
      ]
    );
  }

  #[test]
  fn plan_defers_orphan_first_line() {
    // Arrange
    let lines = vec![test_line(), test_line(), test_line()];

    // Act
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      pt(46.0),
      false,
      pt(12.0),
      pt(10.0),
      pt(50.0),
      &no_footnotes(3),
      Length::ZERO,
      no_charges(),
      true,
      false,
    );

    // Assert
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: true,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
      ]
    );
  }

  #[test]
  fn plan_pulls_widow_last_line_back() {
    // Arrange
    let lines = vec![
      test_line(),
      test_line(),
      test_line(),
      test_line(),
      test_line(),
    ];

    // Act
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      pt(10.0),
      false,
      pt(12.0),
      pt(10.0),
      pt(50.0),
      &no_footnotes(5),
      Length::ZERO,
      no_charges(),
      true,
      false,
    );

    // Assert
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(10.0),
          starts_region: true,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
      ]
    );
  }
}
